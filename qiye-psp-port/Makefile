TARGET = vdingoo
OBJS = main.o ccdl.o hle.o

CFLAGS = -O2 -Wall -Wextra -G0
LIBS = -lpspdisplay -lpspge -lpspctrl -lpspaudio -lm

EXTRA_TARGETS = EBOOT.PBP
PSP_EBOOT_TITLE = vDingoo - qiye
PSP_EBOOT_ICON = NULL

PSPSDK=$(shell psp-config --pspsdk-path)
include $(PSPSDK)/lib/build.mak
