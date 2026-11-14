// userland/hello_trixie.c
// Genuine C user-space application compiled by GCC into an x86_64 Linux ELF64 binary,
// packaged into a Debian 13 (Trixie) .deb archive, and executed in Ring 3 on ZenithOS.

typedef unsigned long uint64_t;
typedef long int64_t;
typedef unsigned int uint32_t;
typedef unsigned short uint16_t;
typedef unsigned char uint8_t;

#define SYS_READ            0
#define SYS_WRITE           1
#define SYS_CLOSE           3
#define SYS_IOCTL           16
#define SYS_UNAME           63
#define SYS_EXIT_GROUP      231
#define SYS_OPENAT          257
#define SYS_EPOLL_PWAIT     281
#define SYS_EPOLL_CREATE1   291

#define AT_FDCWD            (-100)
#define O_RDONLY            0
#define O_RDWR              2
#define FBIOGET_VSCREENINFO 0x4600

static inline int64_t syscall1(uint64_t n, uint64_t a1) {
    int64_t ret;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(n), "D"(a1) : "rcx", "r11", "memory");
    return ret;
}

static inline int64_t syscall3(uint64_t n, uint64_t a1, uint64_t a2, uint64_t a3) {
    int64_t ret;
    __asm__ volatile("syscall"
                     : "=a"(ret)
                     : "a"(n), "D"(a1), "S"(a2), "d"(a3)
                     : "rcx", "r11", "memory");
    return ret;
}

static inline int64_t syscall4(uint64_t n, uint64_t a1, uint64_t a2, uint64_t a3, uint64_t a4) {
    int64_t ret;
    register uint64_t r10 __asm__("r10") = a4;
    __asm__ volatile("syscall"
                     : "=a"(ret)
                     : "a"(n), "D"(a1), "S"(a2), "d"(a3), "r"(r10)
                     : "rcx", "r11", "memory");
    return ret;
}

static void z_print(const char *s) {
    uint64_t len = 0;
    while (s[len] != '\0') {
        len++;
    }
    syscall3(SYS_WRITE, 1, (uint64_t)s, len);
}

struct linux_utsname {
    char sysname[65];
    char nodename[65];
    char release[65];
    char version[65];
    char machine[65];
    char domainname[65];
};

__attribute__((noreturn)) void _start(void) {
    // 1. Write verification signature "DEB3" to shared VMO address 0x10000600
    volatile uint32_t *sig_ptr = (volatile uint32_t *)0x10000600UL;
    *sig_ptr = 0x33424544U; // "DEB3" in little-endian

    // 2. Query kernel identity via Linux SYS_uname(63)
    struct linux_utsname uts;
    syscall1(SYS_UNAME, (uint64_t)&uts);
    z_print("[C-APP /usr/bin/hello-deb] GCC ELF64 running on ");
    z_print(uts.sysname);
    z_print(" ");
    z_print(uts.release);
    z_print(" (");
    z_print(uts.machine);
    z_print(")\n");

    // 3. Read /etc/debian_version from ZenithFS
    int64_t ver_fd = syscall4(SYS_OPENAT, (uint64_t)AT_FDCWD, (uint64_t)"/etc/debian_version", O_RDONLY, 0);
    if (ver_fd >= 0) {
        char ver_buf[16] = {0};
        syscall3(SYS_READ, (uint64_t)ver_fd, (uint64_t)ver_buf, 8);
        syscall1(SYS_CLOSE, (uint64_t)ver_fd);
        z_print("[C-APP /usr/bin/hello-deb] ZenithFS /etc/debian_version = ");
        z_print(ver_buf);
    }

    // 4. Poll and read Linux evdev (/dev/input/event0)
    int64_t epfd = syscall1(SYS_EPOLL_CREATE1, 0);
    int64_t ev_fd = syscall4(SYS_OPENAT, (uint64_t)AT_FDCWD, (uint64_t)"/dev/input/event0", O_RDONLY, 0);
    syscall4(SYS_EPOLL_PWAIT, (uint64_t)epfd, 0x10000900UL, 1, 0);
    syscall3(SYS_READ, (uint64_t)ev_fd, 0x10000920UL, 24);
    syscall1(SYS_CLOSE, (uint64_t)ev_fd);
    syscall1(SYS_CLOSE, (uint64_t)epfd);

    // 5. Open /dev/fb0, query resolution, and draw 4 pixels
    int64_t fb_fd = syscall4(SYS_OPENAT, (uint64_t)AT_FDCWD, (uint64_t)"/dev/fb0", O_RDWR, 0);
    if (fb_fd >= 0) {
        syscall3(SYS_IOCTL, (uint64_t)fb_fd, FBIOGET_VSCREENINFO, 0x10000800UL);
        uint32_t pixels[4] = {0x00FFFFFFU, 0x0060CDFFU, 0x000078D4U, 0x001F2128U};
        syscall3(SYS_WRITE, (uint64_t)fb_fd, (uint64_t)pixels, 16);
        syscall1(SYS_CLOSE, (uint64_t)fb_fd);
    }

    // 6. Terminate cleanly with exit code 0x66
    syscall1(SYS_EXIT_GROUP, 0x66);
    __builtin_unreachable();
}
