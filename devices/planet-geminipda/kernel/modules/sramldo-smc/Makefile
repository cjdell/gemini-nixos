# Out-of-tree module for the gemini 6.6 kernel (A72 bring-up SMC).
#
# Primary build path: the kernel derivation (../../default.nix) copies
# this dir into the kernel build dir and runs
# `make M=sramldo-smc-src modules` from there (postInstall), which does
# not use this wrapper (KDIR unset -> plain obj-m form).
#
# Manual rebuild against an existing build tree:
#   make KDIR=<kernel-build-dir>
obj-m := sramldo-smc.o

KDIR ?=

ifneq ($(KDIR),)
all:
	$(MAKE) -C $(KDIR) ARCH=arm64 M=$(CURDIR) modules
clean:
	$(MAKE) -C $(KDIR) ARCH=arm64 M=$(CURDIR) clean
.PHONY: all clean
endif
