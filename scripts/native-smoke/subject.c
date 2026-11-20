/* Independent original operation; helper exit zero is not an allow receipt. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

static int map_shared_write(const char *path)
{
    int fd = open(path, O_RDWR | O_CLOEXEC);
    if (fd < 0) return -1;
    char *mapped = mmap(NULL, 1, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    int result = -1, error = errno;
    if (mapped != MAP_FAILED) {
        *mapped = 'X';
        result = msync(mapped, 1, MS_SYNC);
        error = errno;
        munmap(mapped, 1);
    }
    close(fd);
    errno = error;
    return result;
}

static int truncate_open_file(const char *path, int at_open)
{
    int fd = open(path, O_RDWR | O_CLOEXEC | (at_open ? O_TRUNC : 0));
    if (fd < 0) return -1;
    int result = at_open ? 0 : ftruncate(fd, 0);
    int error = errno;
    close(fd);
    errno = error;
    return result;
}

int main(int argc, char **argv)
{
    int result = -1, error;
    int waiting = argc > 1 && !strcmp(argv[argc - 1], "--wait");
    int arguments = argc - waiting;
    if (arguments < 3 || arguments > 4) return 2;
    const char *operation = argv[1], *path = argv[2];
    int pair = !strcmp(operation, "rename") || !strcmp(operation, "exchange");
    if (arguments != (pair ? 4 : 3) || (!pair && strcmp(operation, "open") &&
        strcmp(operation, "mkdir") && strcmp(operation, "unlink") && strcmp(operation, "rmdir") &&
        strcmp(operation, "truncate-path") && strcmp(operation, "truncate-fd") &&
        strcmp(operation, "truncate-open") && strcmp(operation, "map-shared-write"))) return 2;
    if (waiting) {
        char signal;
        printf("{\"phase\":\"ready\",\"pid\":%ld}\n", (long)getpid());
        fflush(stdout);
        if (read(STDIN_FILENO, &signal, 1) != 1) return 3;
    }
    errno = 0;
    if (!strcmp(operation, "open")) {
        int fd = open(path, O_RDONLY | O_CLOEXEC);
        error = fd < 0 ? errno : 0;
        if (fd >= 0) { close(fd); result = 0; }
    } else {
        if (!strcmp(operation, "mkdir")) { umask(0); result = mkdir(path, 0755); }
        else if (!strcmp(operation, "unlink")) result = unlink(path);
        else if (!strcmp(operation, "rmdir")) result = rmdir(path);
        else if (!strcmp(operation, "truncate-path")) result = truncate(path, 2);
        else if (!strcmp(operation, "truncate-fd")) result = truncate_open_file(path, 0);
        else if (!strcmp(operation, "truncate-open")) result = truncate_open_file(path, 1);
        else if (!strcmp(operation, "map-shared-write")) result = map_shared_write(path);
        else result = renameat2(AT_FDCWD, path, AT_FDCWD, argv[3], !strcmp(operation, "exchange") ? RENAME_EXCHANGE : 0);
        error = result < 0 ? errno : 0;
    }
    printf("{\"pid\":%ld,\"operation\":\"%s\",\"result\":%d,\"errno\":%d}\n", (long)getpid(), operation, result, error);
    return 0;
}
