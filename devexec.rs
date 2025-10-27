// SPDX-License-Identifier: GPL-2.0

//! Rust kernel module that adds the /dev/exec misc device

use core::pin::Pin;

use kernel::{
    alloc::KBox,
    bindings, c_str,
    error::Error,
    fs::Kiocb,
    iov::IovIterSource,
    macros::{module, vtable},
    new_mutex, pr_info,
    prelude::*,
    sync::Mutex,
    try_pin_init,
    uapi::{call_usermodehelper_exec, call_usermodehelper_setup, UMH_WAIT_PROC},
    ThisModule,
};

module! {
   type: DevExecModule,
   name: "devexec",
   authors: ["Connor Sample"],
   description: "Rust kernel module that adds the /dev/exec misc device",
   license: "GPL",
}

#[allow(improper_ctypes)]
unsafe extern "C" {
    /// get an unlinked file living in tmpfs
    unsafe fn shmem_file_setup(
        // name for dentry (to be seen in /proc/<pid>/maps)
        name: *const c_char,
        // size to be set for the file
        size: i64,
        // VM_NORESERVE supresses pre-accounting of the entire object size
        flags: c_ulong,
    ) -> *mut bindings::file;
}

/// Module struct that registers the misc device
#[pin_data(PinnedDrop)]
struct DevExecModule {
    #[pin]
    _miscdev: kernel::miscdevice::MiscDeviceRegistration<DevExecDevice>,
}

impl kernel::InPlaceModule for DevExecModule {
    // init is called when the module is loaded
    fn init(_module: &'static ThisModule) -> impl PinInit<Self, Error> {
        pr_info!("devexec module (init)\n");

        let options = kernel::miscdevice::MiscDeviceOptions {
            // set the misc device to /dev/exec
            name: kernel::c_str!("exec"),
        };

        // attempt to initialize the module
        try_pin_init!(Self {
            _miscdev <- kernel::miscdevice::MiscDeviceRegistration::register(options),
        })
    }
}

#[pinned_drop]
impl PinnedDrop for DevExecModule {
    // called when the module is unloaded
    fn drop(self: Pin<&mut Self>) {
        pr_info!("devexec module (exit)\n");
    }
}

/// Device state: holds a byte buffer written by userspace
#[pin_data(PinnedDrop)]
struct DevExecDevice {
    #[pin]
    data: Mutex<KVVec<u8>>,
}

/// callback function passed to call_usermodehelper_setup.
///
/// This is executed in the context of the child process setup. It installs the memfd file onto fd
/// 3 of the spawned subprocess to that the helper can access the in memory file via
///   /proc/self/fd/3
#[no_mangle]
unsafe extern "C" fn kmod_devexec_init(
    info: *mut kernel::uapi::subprocess_info,
    _cred: *mut kernel::uapi::cred,
) -> c_int {
    const EXEC_FD: u32 = 3;

    // `subprocess_info->data` is used to pass our memfd pointer.
    // We recast it to a `struct file *`
    let file = unsafe { (*info).data as *mut kernel::uapi::file };

    unsafe { kernel::uapi::fd_install(EXEC_FD, file) };

    0
}

// Implementation of the misc device
#[vtable]
impl kernel::miscdevice::MiscDevice for DevExecDevice {
    type Ptr = Pin<KBox<Self>>;

    // Opening this file allocates the device's state and returns it
    fn open(
        _file: &kernel::fs::File,
        _misc: &kernel::miscdevice::MiscDeviceRegistration<Self>,
    ) -> Result<Pin<KBox<Self>>> {
        pr_info!("devexec: device opened\n");

        // create a DevExecDevice with an empty buffer protected by a mutex
        KBox::try_pin_init(
            try_pin_init! {
                DevExecDevice {
                    data <- new_mutex!(KVVec::new())
                }
            },
            GFP_KERNEL,
        )
    }

    // Copies the provided iterator into the device's buffer when userspace writes to /dev/exec
    fn write_iter(kiocb: Kiocb<'_, Self::Ptr>, iov: &mut IovIterSource<'_>) -> Result<usize> {
        let file = kiocb.file();
        let mut guard = file.data.lock();
        // copy the iov iterator into the vector, allocating with GFP_KERNEL
        let len = iov.copy_from_iter_vec(&mut guard, GFP_KERNEL)?;

        pr_info!("devexec: write {} bytes\n", len);
        Ok(len)
    }

    // This is called when the device is closed. Execution is attempted
    fn release(device: Self::Ptr, _file: &kernel::fs::File) {
        pr_info!("devexec: device closed, attempting execution\n");

        // take the buffer out of the device
        let buffer = core::mem::take(&mut *device.data.lock());
        if buffer.is_empty() {
            pr_warn!("devexec: buffer is empty, nothing to execute\n");
            return;
        }

        // name for the shmem file
        let name = c_str!("kmod_devexec");
        let mem_file_ptr = unsafe { shmem_file_setup(name.as_ptr(), 0, 0) };

        // check for errors while creating the file
        if unsafe { bindings::IS_ERR(mem_file_ptr as *const c_void) } {
            let err = unsafe { bindings::PTR_ERR(mem_file_ptr as *const c_void) };
            pr_err!("devexec: shmem_file_setup failed: {}\n", err);
            return;
        }

        // write the buffer into the shmem file
        let mut offset: i64 = 0;
        let ret = unsafe {
            kernel::uapi::kernel_write(
                mem_file_ptr.cast(),
                buffer.as_ptr().cast(),
                buffer.len(),
                &mut offset,
            )
        };

        if ret < 0 {
            pr_err!("devexec: kernel_write to memfd failed: {}\n", ret);
            // drop the file reference explicitly if write failed
            unsafe { bindings::fput(mem_file_ptr) };
            return;
        }

        pr_info!("devexec: wrote {} bytes to memfd\n", ret);

        // point to our FD that will be created
        // SAFETY: this ends with a \0
        let mut path_bytes = *b"/proc/self/fd/3\0";
        let path_ptr: *mut u8 = path_bytes.as_mut_ptr() as *mut u8;
        let mut argv = [path_ptr, core::ptr::null_mut()];
        let mut envp = [core::ptr::null_mut()];

        // set up the subprocess, passing mem_file_ptr as `data` so that it can be installed into
        // the child's fd table.
        // using GFP_KERNEL for allocation
        let sub_info = unsafe {
            call_usermodehelper_setup(
                path_ptr,
                argv.as_mut_ptr(),
                envp.as_mut_ptr(),
                bindings::GFP_KERNEL,
                Some(kmod_devexec_init),
                None,
                mem_file_ptr as *mut c_void,
            )
        };

        if sub_info.is_null() {
            pr_err!("devexec: call_usermodehelper_setup failed\n");
            // drop the file reference explicitly if write failed
            unsafe { bindings::fput(mem_file_ptr) };
            return;
        }

        // execute the userspace helper process that wait for it to finish
        let ret = unsafe { call_usermodehelper_exec(sub_info, UMH_WAIT_PROC.try_into().unwrap()) };
        pr_info!("devexec: usermode helper returned {}\n", ret);
    }
}

#[pinned_drop]
impl PinnedDrop for DevExecDevice {
    fn drop(self: Pin<&mut Self>) {
        core::mem::drop(self);
    }
}
