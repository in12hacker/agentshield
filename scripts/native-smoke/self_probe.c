/* Independent original objects prepared before self-protection is enabled.
 * This fixture never opens the controller endpoint or changes driver state. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/xattr.h>
#include <unistd.h>

#define OBJECT_BYTES 4096
static const char attribute[] = "user.as_probe";
static const char initial_attribute[] = "before";
static unsigned char original[OBJECT_BYTES];
static int object_fd = -1, alias_fd = -1;
static char object_path[PATH_MAX], created_path[PATH_MAX], renamed_path[PATH_MAX];
static const char *alias_path;
static struct stat initial;
static unsigned cases, failures;

static const char *boolean(bool value) { return value ? "true" : "false"; }
static void setup_failed(const char *operation)
{
    int error = errno;
    printf("{\"phase\":\"setup_failed\",\"operation\":\"%s\",\"errno\":%d}\n",
           operation, error);
    exit(1);
}
static void join(char *out, size_t capacity, const char *directory, const char *name)
{
    int n = snprintf(out, capacity, "%s/%s", directory, name);
    if (n < 0 || (size_t)n >= capacity) { errno = ENAMETOOLONG; setup_failed("path"); }
}
static bool content_unchanged(void)
{
    unsigned char bytes[OBJECT_BYTES];
    struct stat now;
    return pread(object_fd, bytes, sizeof(bytes), 0) == sizeof(bytes) &&
           !fstat(object_fd, &now) && now.st_size == sizeof(bytes) &&
           !memcmp(bytes, original, sizeof(bytes));
}
static bool same_object(const char *path)
{
    struct stat now;
    return !lstat(path, &now) && now.st_dev == initial.st_dev && now.st_ino == initial.st_ino;
}
static bool absent(const char *path)
{
    struct stat st;
    errno = 0;
    return lstat(path, &st) < 0 && errno == ENOENT;
}
static bool mode_unchanged(void)
{
    struct stat now;
    return !fstat(object_fd, &now) && (now.st_mode & 07777) == (initial.st_mode & 07777);
}
static bool attribute_unchanged(void)
{
    char value[32];
    ssize_t n = fgetxattr(object_fd, attribute, value, sizeof(value));
    return n == sizeof(initial_attribute) - 1 &&
           !memcmp(value, initial_attribute, sizeof(initial_attribute) - 1);
}
static void result(const char *name, long rc, int error, bool state)
{
    bool content = content_unchanged();
    bool pass = rc < 0 && error == EPERM && content && state;
    cases++;
    if (!pass) failures++;
    printf("{\"case\":\"%s\",\"rc\":%ld,\"errno\":%d,\"expected_errno\":%d,"
           "\"content_unchanged\":%s,\"object_state_unchanged\":%s,\"passed\":%s}\n",
           name, rc, error, EPERM, boolean(content), boolean(state), boolean(pass));
}
static void attempt_write(int fd, const char *name, off_t offset)
{
    unsigned char replacement = 'X';
    errno = 0;
    ssize_t rc = pwrite(fd, &replacement, 1, offset);
    int error = rc < 0 ? errno : 0;
    result(name, rc, error, same_object(object_path) && same_object(alias_path));
}
int main(int argc, char **argv)
{
    void *read_mapping, *new_mapping;
    int rc, error, fd;
    char command;
    if (argc != 3 || argv[1][0] != '/' || argv[2][0] != '/') return 2;
    setvbuf(stdout, NULL, _IOLBF, 0);
    alias_path = argv[2];
    join(object_path, sizeof(object_path), argv[1], "original");
    join(created_path, sizeof(created_path), argv[1], "created");
    join(renamed_path, sizeof(renamed_path), argv[1], "renamed");
    if (mkdir(argv[1], 0700)) setup_failed("mkdir");
    object_fd = open(object_path, O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC, 0640);
    if (object_fd < 0) setup_failed("open_original");
    memcpy(original, "AS_SELF_PROBE_ORIGINAL\n", 23);
    if (write(object_fd, original, sizeof(original)) != sizeof(original)) setup_failed("seed_content");
    if (fchmod(object_fd, 0640)) setup_failed("seed_mode");
    if (fsetxattr(object_fd, attribute, initial_attribute, sizeof(initial_attribute) - 1, 0))
        setup_failed("seed_xattr");
    if (link(object_path, alias_path)) setup_failed("link_external_alias");
    alias_fd = open(alias_path, O_RDWR | O_CLOEXEC);
    if (alias_fd < 0) setup_failed("open_external_alias");
    read_mapping = mmap(NULL, OBJECT_BYTES, PROT_READ, MAP_SHARED, object_fd, 0);
    if (read_mapping == MAP_FAILED) setup_failed("prepare_readonly_mapping");
    if (fstat(object_fd, &initial)) setup_failed("baseline_stat");
    printf("{\"phase\":\"ready\",\"pid\":%ld,\"bytes\":%d,\"mode\":%u,"
           "\"device\":%llu,\"inode\":%llu,\"links\":%llu,\"prior_mapping\":\"shared_readonly\"}\n",
           (long)getpid(), OBJECT_BYTES, (unsigned)(initial.st_mode & 07777),
           (unsigned long long)initial.st_dev, (unsigned long long)initial.st_ino,
           (unsigned long long)initial.st_nlink);
    do { rc = read(STDIN_FILENO, &command, 1); } while (rc < 0 && errno == EINTR);
    if (rc != 1 || command != 'G') { errno = ECANCELED; setup_failed("await_enable_signal"); }

    attempt_write(object_fd, "existing_fd_write", 0);
    attempt_write(alias_fd, "external_alias_existing_fd_write", 1);

    errno = 0;
    new_mapping = mmap(NULL, OBJECT_BYTES, PROT_READ | PROT_WRITE, MAP_SHARED, object_fd, 0);
    error = new_mapping == MAP_FAILED ? errno : 0;
    if (new_mapping != MAP_FAILED) {
        ((volatile unsigned char *)new_mapping)[2] = 'M';
        (void)msync(new_mapping, OBJECT_BYTES, MS_SYNC);
        munmap(new_mapping, OBJECT_BYTES);
    }
    result("new_shared_writable_mapping", new_mapping == MAP_FAILED ? -1 : 0, error, true);

    errno = 0;
    rc = mprotect(read_mapping, OBJECT_BYTES, PROT_READ | PROT_WRITE);
    error = rc < 0 ? errno : 0;
    if (!rc) {
        ((volatile unsigned char *)read_mapping)[3] = 'P';
        (void)msync(read_mapping, OBJECT_BYTES, MS_SYNC);
    }
    result("readonly_mapping_write_grant", rc, error, true);

    errno = 0;
    rc = fchmod(object_fd, 0600); error = rc < 0 ? errno : 0;
    result("change_mode", rc, error, mode_unchanged());
    errno = 0;
    rc = fsetxattr(object_fd, attribute, "after", 5, 0); error = rc < 0 ? errno : 0;
    result("set_xattr", rc, error, attribute_unchanged());
    errno = 0;
    rc = fremovexattr(object_fd, attribute); error = rc < 0 ? errno : 0;
    result("remove_xattr", rc, error, attribute_unchanged());

    errno = 0;
    fd = open(alias_path, O_WRONLY | O_CLOEXEC); error = fd < 0 ? errno : 0;
    rc = fd < 0 ? -1 : 0;
    if (fd >= 0) close(fd);
    result("external_alias_write_open", rc, error, same_object(alias_path));

    errno = 0;
    fd = open(created_path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    error = fd < 0 ? errno : 0; rc = fd < 0 ? -1 : 0;
    if (fd >= 0) close(fd);
    result("create", rc, error, absent(created_path));
    errno = 0;
    rc = rename(object_path, renamed_path); error = rc < 0 ? errno : 0;
    result("rename", rc, error, same_object(object_path) && absent(renamed_path));
    errno = 0;
    rc = unlink(object_path); error = rc < 0 ? errno : 0;
    result("unlink", rc, error, same_object(object_path) && same_object(alias_path));

    bool bytes = content_unchanged(), mode = mode_unchanged(), xattr = attribute_unchanged();
    bool names = same_object(object_path) && same_object(alias_path) &&
                 absent(created_path) && absent(renamed_path);
    bool pass = !failures && bytes && mode && xattr && names;
    printf("{\"phase\":\"summary\",\"cases\":%u,\"failed_cases\":%u,\"content_unchanged\":%s,"
           "\"mode_unchanged\":%s,\"xattr_unchanged\":%s,\"names_unchanged\":%s,\"passed\":%s}\n",
           cases, failures, boolean(bytes), boolean(mode), boolean(xattr), boolean(names), boolean(pass));
    munmap(read_mapping, OBJECT_BYTES);
    close(alias_fd); close(object_fd);
    return pass ? 0 : 1;
}
