# devexec

This is possibly the worst thing you will ever come across. Devexec is a kernel module written in Rust that allows users to arbitrarily write executables directly into the `/dev/exec` device file. Once `/dev/exec` is closed, it will take its content and attempt to execute it in userspace.

## Running

Building and running this kernel module is pretty straightforward. I really have no idea how to make this work on an existing kernel, so I'll give the instructions that I used to get it up and running.

### Kernel Setup

1. Create a new VM. I used Debian 13 for my testing.

```sh
# Install build dependencies
apt install build-essential libssl-dev python3 flex bison bc libncurses-dev gawk openssl libssl-dev libelf-dev libudev-dev libpci-dev libiberty-dev autoconf llvm clang lld git curl

# Install Rust, rust-src, and bindgen
# For non-debian distros, check https://docs.kernel.org/rust/quick-start.html#distributions
apt install rustc rust-src bindgen

# In the future, you can probably get away with downloading the 6.18 tarball once it comes out, but for reproducability, this is what I built on
# Clone Linux
cd ~
curl -LO "https://github.com/torvalds/linux/archive/refs/tags/v6.18-rc3.tar.gz"
tar xvf v6.18-rc3.tar.gz && rm v6.18-rc3.tar.gz && cd linux-6.18-rc3/

# Make Rust Available
make LLVM=1 rustavailable
make LLVM=1 defconfig
```

Now, run `make LLVM=1 menuconfig`. Navigate to `General Setup` and make sure `Rust support` is ticked. Then, use the arrow keys to select `< Exit >`, then `< Exit >` again, then `< Yes >` to save. Now, we can build and install the kernel:

```sh
# change 8 to be your number of cores
make LLVM=1 -j8
make LLVM=1 modules_install
make LLVM=1 install
reboot
```

### Module Installation

```sh
cd ~
git clone https://github.com/TabulateJarl8/devexec.git && cd devexec
# replace ../linux with the path to your kernel source
make KDIR=../linux-6.18-rc3/ LLVM=1

# load module
insmod devexec.ko

# OPTIONAL: check dmesg logs with
# dmesg

# test code
gcc demo.c
cat a.out > /dev/exec

# unload module
rmmod devexec
```

## How It Works

1. Kernel module is loaded
2. User writes an executable to `/dev/exec`
3. User closes the file
4. The module:

- Creates an in-kernel anonymous memory file using `shmem_file_setup`
- Copies the written bytes into the file
- Passes the file as FD 3 to a userspace helper
- Executes it using `call_usermodehelper`

## Why?

I wanted to learn how to make a Rust kernel module and this seemed like the worst idea I could come up with.

## Security

## Development

Please don't develop this. If you do, you can set up a `rust-project.json` by running the following command:

```sh
make -C ../linux M=$PWD rust-analyzer
```
