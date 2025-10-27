# SPDX-License-Identifier: GPL-2.0

KDIR ?= /lib/modules/`uname -r`/build

default:
	$(MAKE) -C $(KDIR) M=$$PWD

modules_install: default
	$(MAKE) -C $(KDIR) M=$$PWD modules_install

clean:
	rm -f *.ko *.mod *.mod.c *.o *.rmeta Module.symvers modules.order .*module* .*.cmd .*.d *.out
	rm -rf .tmp_*
