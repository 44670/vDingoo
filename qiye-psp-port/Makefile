TARGET = vdingoo
OBJS = main.o ccdl.o hle.o

CFLAGS = -O2 -Wall -Wextra -G0
LDFLAGS += -Wl,--section-start,.game_buf=0x08A00000
LIBS = -lpspaudio -lm

EXTRA_TARGETS = EBOOT.PBP
PSP_EBOOT_TITLE = vDingoo - qiye
PSP_EBOOT_ICON = NULL

PSPSDK=$(shell psp-config --pspsdk-path)
include $(PSPSDK)/lib/build.mak

clean:
	rm -f $(OBJS) $(TARGET).elf $(TARGET)_strip.elf EBOOT.PBP PARAM.SFO
