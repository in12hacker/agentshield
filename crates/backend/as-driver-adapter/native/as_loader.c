/* SPDX-License-Identifier: BSD-2-Clause */
/* libbpf link lifetime: a lost parent preserves pinned enforcement; explicit U retires it. */
#include <bpf/bpf.h>
#include <bpf/libbpf.h>
#include "as_protocol.h"
#include <linux/magic.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/vfs.h>
#include <unistd.h>

static int directory(const char *path)
{
    struct stat st;
    if (mkdir(path, 0700) && errno != EEXIST)
        return -1;
    if (lstat(path, &st) || !S_ISDIR(st.st_mode) || st.st_uid != 0 || (st.st_mode & 077))
        return -1;
    return 0;
}
static int same_program(struct bpf_link *link, struct bpf_program *program)
{
    struct bpf_link_info li = {0};
    struct bpf_prog_info actual = {0}, expected = {0};
    __u32 len = sizeof(li);
    int fd, rc;
    if (bpf_obj_get_info_by_fd(bpf_link__fd(link), &li, &len) ||
        li.type != BPF_LINK_TYPE_TRACING || li.tracing.attach_type != BPF_LSM_MAC)
        return 0;
    fd = bpf_prog_get_fd_by_id(li.prog_id);
    if (fd < 0)
        return 0;
    len = sizeof(actual);
    rc = bpf_obj_get_info_by_fd(fd, &actual, &len);
    close(fd);
    len = sizeof(expected);
    if (rc || bpf_obj_get_info_by_fd(bpf_program__fd(program), &expected, &len))
        return 0;
    return actual.type == expected.type &&
           actual.attach_btf_id == expected.attach_btf_id &&
           actual.attach_btf_obj_id == expected.attach_btf_obj_id &&
           !memcmp(actual.tag, expected.tag, sizeof(actual.tag)) &&
           !memcmp(actual.name, expected.name, sizeof(actual.name));
}
int main(int argc, char **argv)
{
    static const char *const names[] = {
        "as_file_open", "as_exec", "as_connect", "as_sendmsg",
        "as_self_permission", "as_self_file_truncate", "as_truncate_path",
        "as_self_chmod", "as_self_chown", "as_file_unlink", "as_file_rmdir",
        "as_file_mkdir", "as_file_create", "as_self_create", "as_self_symlink", "as_self_link",
        "as_file_rename", "as_inode_setattr", "as_mmap_shared_write", "as_self_mprotect",
        "as_self_mount", "as_self_move_mount", "as_self_umount",
        "as_self_setxattr", "as_self_removexattr", "as_self_set_acl",
        "as_self_remove_acl", "as_self_file_setattr", "as_file_close"
    };
    enum { program_count = sizeof(names) / sizeof(names[0]) };
    struct bpf_link *links[program_count] = {0};
    struct bpf_object *object = NULL;
    struct statfs fs;
    struct stat st;
    char instance[128], paths[program_count][192];
    unsigned fresh = 0;
    int rc = 1, existing = 0, retire = 0;
    if (argc != 3 || strlen(argv[2]) != 32) {
        fputs("usage: as-bpf-loader OBJECT DRIVER_INSTANCE_HEX\n", stderr);
        return 2;
    }
    for (unsigned i = 0; i < 32; i++)
        if (!((argv[2][i] >= '0' && argv[2][i] <= '9') ||
              (argv[2][i] >= 'a' && argv[2][i] <= 'f')))
            return 2;
    if (geteuid() != 0 || statfs("/sys/fs/bpf", &fs) ||
        (unsigned long)fs.f_type != BPF_FS_MAGIC || directory(AS_BPF_PIN_ROOT))
        return 1;
    if (snprintf(instance, sizeof(instance), AS_BPF_PIN_ROOT "/%s", argv[2]) >= (int)sizeof(instance) || directory(instance))
        return 1;
    for (unsigned i = 0; i < program_count; i++) {
        if (snprintf(paths[i], sizeof(paths[i]), "%s/%s", instance, names[i]) >= (int)sizeof(paths[i]))
            return 1;
        if (!lstat(paths[i], &st)) {
            if (!S_ISREG(st.st_mode) || st.st_uid != 0 || (st.st_mode & 077))
                return 1;
            existing++;
        } else if (errno != ENOENT) {
            return 1;
        }
    }
    /* A partial/unknown set is never silently adopted or deleted. */
    if (existing && existing != program_count)
        return 1;
    {
        DIR *dir = opendir(instance);
        struct dirent *entry;
        if (!dir)
            return 1;
        while ((entry = readdir(dir))) {
            unsigned i;
            if (!strcmp(entry->d_name, ".") || !strcmp(entry->d_name, ".."))
                continue;
            for (i = 0; i < program_count; i++)
                if (!strcmp(entry->d_name, names[i]))
                    break;
            if (i == program_count) {
                closedir(dir);
                return 1;
            }
        }
        closedir(dir);
    }
    object = bpf_object__open_file(argv[1], NULL);
    if (libbpf_get_error(object))
        return 1;
    if (bpf_object__load(object))
        goto done;
    {
        struct bpf_program *program;
        unsigned seen = 0;
        bpf_object__for_each_program(program, object) {
            unsigned i;
            for (i = 0; i < program_count; i++)
                if (!strcmp(bpf_program__name(program), names[i]))
                    break;
            if (i == program_count)
                goto done;
            seen++;
        }
        if (seen != program_count)
            goto done;
    }
    for (unsigned i = 0; i < program_count; i++) {
        struct bpf_program *program = bpf_object__find_program_by_name(object, names[i]);
        if (!program)
            goto done;
        if (existing) {
            links[i] = bpf_link__open(paths[i]);
            if (libbpf_get_error(links[i])) { links[i] = NULL; goto done; }
            if (!same_program(links[i], program))
                goto done;
        } else {
            links[i] = bpf_program__attach_lsm(program);
            if (libbpf_get_error(links[i])) { links[i] = NULL; goto done; }
            if (bpf_link__pin(links[i], paths[i]))
                goto done;
            fresh++;
            if (chmod(paths[i], 0600))
                goto done;
        }
    }
    if (puts("AS-BPF-READY") == EOF || fflush(stdout))
        goto done;
    for (;;) {
        char command;
        ssize_t count = read(STDIN_FILENO, &command, 1);
        if (count < 0 && errno == EINTR)
            continue;
        if (count == 1 && command == 'U')
            retire = 1;
        if (count >= 0) rc = 0;
        break;
    }
    /* EOF is an unexpected control loss: leave the kernel pins alive. */
    fresh = 0;
done:
    for (unsigned i = 0; i < program_count; i++) {
        if (links[i] && (retire || i < fresh)) {
            if (bpf_link__unpin(links[i]))
                rc = 1;
        }
        bpf_link__destroy(links[i]);
    }
    if (retire || fresh)
        (void)rmdir(instance);
    bpf_object__close(object);
    return rc;
}
