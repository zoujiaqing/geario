# Builds the C examples against the release staticlib.
#
#   cargo build -p geario-ffi --release
#   make -C geario-ffi/examples

TARGET_DIR ?= ../../target/release
CFLAGS += -I../include -Wall -Wextra
LDLIBS = $(TARGET_DIR)/libgeario_ffi.a

UNAME := $(shell uname -s)
ifeq ($(UNAME),Darwin)
LDLIBS += -framework CoreFoundation -framework Security -lc++
else
LDLIBS += -lpthread -ldl -lm
endif

all: version

version: version.c
	$(CC) $(CFLAGS) -o $@ $< $(LDLIBS)

clean:
	rm -f version

.PHONY: all clean
