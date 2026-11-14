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
#define SYS_GETUID          102
#define SYS_EXIT_GROUP      231
#define SYS_OPENAT          257
#define SYS_EPOLL_PWAIT     281
#define SYS_EPOLL_CREATE1   291

#define AT_FDCWD            (-100)
#define O_RDONLY            0
#define O_WRONLY            1
#define O_RDWR              2
#define O_CREAT             0x40
#define FBIOGET_VSCREENINFO 0x4600

__asm__(
    ".global _start\n"
    "_start:\n"
    "    andq $-16, %rsp\n"
    "    call c_main\n"
);

static inline int64_t syscall0(uint64_t n) {
    int64_t ret;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(n) : "rcx", "r11", "memory");
    return ret;
}

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

__attribute__((noreturn, used)) void c_main(void) {
    volatile uint32_t *sig_ptr = (volatile uint32_t *)0x10000600UL;
    *sig_ptr = 0x33424544U; // "DEB3"

    // 1. Verify unprivileged UID=1000 with Z-Cap Passwordless Security
    int64_t uid = syscall0(SYS_GETUID);
    if (uid == 1000) {
        z_print("[C-APP /usr/bin/hello-deb] Running as UID=1000 (Z-Cap Passwordless Mode)\n");
    }

    // 2. Create and write a new file `/home/user/zenith.txt` on ZenithFS without sudo
    int64_t note_fd = syscall4(
        SYS_OPENAT,
        (uint64_t)AT_FDCWD,
        (uint64_t)"/home/user/zenith.txt",
        O_CREAT | O_RDWR,
        0644
    );
    if (note_fd >= 0) {
        const char *payload = "ZFS1_USER_FILE_OK";
        syscall3(SYS_WRITE, (uint64_t)note_fd, (uint64_t)payload, 17);
        syscall1(SYS_CLOSE, (uint64_t)note_fd);
        z_print("[C-APP /usr/bin/hello-deb] Created /home/user/zenith.txt on ZenithFS.\n");
    }

    // 3. Attempt unauthorized write to immutable system file `/etc/debian_version`
    int64_t hack_fd = syscall4(
        SYS_OPENAT,
        (uint64_t)AT_FDCWD,
        (uint64_t)"/etc/debian_version",
        O_WRONLY,
        0
    );
    if (hack_fd == -13) { // -EACCES (Permission Denied)
        z_print("[Z-CAP GUARD] Blocked unauthorized write to /etc/debian_version (-EACCES)!\n");
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
        uint32_t pixels[4];
        pixels[0] = 0x00FFFFFFU;
        pixels[1] = 0x0060CDFFU;
        pixels[2] = 0x000078D4U;
        pixels[3] = 0x001F2128U;
        syscall3(SYS_WRITE, (uint64_t)fb_fd, (uint64_t)pixels, 16);
        syscall1(SYS_CLOSE, (uint64_t)fb_fd);
    }

    syscall1(SYS_EXIT_GROUP, 0x66);
    __builtin_unreachable();
}
