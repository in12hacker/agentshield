/* Disposable-guest family probe. No helper kills: every signal is supplied by AS. */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/uio.h>
#include <unistd.h>

static int write_marker(const char *path)
{
    int fd = open(path, O_WRONLY | O_TRUNC | O_CLOEXEC);
    if (fd < 0) return errno;
    int error = write(fd, "changed", 7) == 7 ? 0 : 6;
    close(fd); return error;
}

static int operation(const char *kind, const char *target, const char *witness)
{
    if (!strcmp(kind, "file")) return write_marker(target);
    if (!strcmp(kind, "create-open")) {
        int fd = open(target, O_WRONLY | O_CREAT | O_CLOEXEC, 0600);
        if (fd < 0) return errno;
        int error = write(fd, "created", 7) == 7 ? 0 : EIO;
        close(fd);
        return error;
    }
    if (!strcmp(kind, "create-mknod"))
        return mknodat(AT_FDCWD, target, S_IFREG | 0600, 0) ? errno : 0;
    if (!strcmp(kind, "exec")) {
        execl(target, target, "mark", witness, (char *)NULL);
        return errno;
    }
    char *end;
    unsigned long port = strtoul(target, &end, 10);
    if (*end || !port || port > 65535) return 2;
    struct sockaddr_in address = {.sin_family=AF_INET,.sin_port=htons(port),.sin_addr={.s_addr=htonl(INADDR_LOOPBACK)}};
    int udp = !strcmp(kind, "sendto") || !strcmp(kind, "sendmmsg");
    int fd = socket(AF_INET, udp ? SOCK_DGRAM : SOCK_STREAM, 0);
    if (fd < 0) return errno;
    int error = 0;
    if (udp) {
        if (!strcmp(kind, "sendmmsg")) {
            struct iovec iov = {.iov_base = "changed", .iov_len = 7};
            struct mmsghdr message = {.msg_hdr = {.msg_name = &address, .msg_namelen = sizeof(address), .msg_iov = &iov, .msg_iovlen = 1}};
            if (sendmmsg(fd, &message, 1, MSG_NOSIGNAL) != 1 || message.msg_len != 7) error = errno ? errno : 6;
        } else if (sendto(fd, "changed", 7, MSG_NOSIGNAL, (void *)&address, sizeof(address)) != 7) error = errno ? errno : 6;
    } else if (connect(fd, (void *)&address, sizeof(address))) error = errno;
    else if (!strcmp(kind, "send-write")) {
        if (write(fd, "changed", 7) != 7) error = errno ? errno : 6;
    } else if (!strcmp(kind, "send-writev") || !strcmp(kind, "sendmsg")) {
        struct iovec iov[] = {{.iov_base = "chan", .iov_len = 4}, {.iov_base = "ged", .iov_len = 3}};
        struct msghdr message = {.msg_iov = iov, .msg_iovlen = 2};
        ssize_t result = !strcmp(kind, "sendmsg") ? sendmsg(fd, &message, MSG_NOSIGNAL) : writev(fd, iov, 2);
        if (result != 7) error = errno ? errno : 6;
    } else if ((!strcmp(kind, "send") || !strcmp(kind, "sendhold")) && send(fd, "changed", 7, MSG_NOSIGNAL) != 7) error = errno ? errno : 6;
    if (!strcmp(kind, "sendhold")) { char release; if (read(STDIN_FILENO, &release, 1) != 1) error = 7; }
    close(fd); return error;
}

int main(int argc, char **argv)
{
    int gate[2], info[2], result[2], parent_status, actor_status;
    pid_t parent, actor;
    char go;
    if (argc == 3 && !strcmp(argv[1], "mark")) return write_marker(argv[2]);
    if (argc != 5 || (strcmp(argv[1], "file") && strcmp(argv[1], "create-open") && strcmp(argv[1], "create-mknod") &&
        strcmp(argv[1], "exec") && strcmp(argv[1], "connect") && strcmp(argv[1], "send") && strcmp(argv[1], "sendhold") && strcmp(argv[1], "sendto") &&
        strcmp(argv[1], "send-write") && strcmp(argv[1], "send-writev") && strcmp(argv[1], "sendmsg") && strcmp(argv[1], "sendmmsg")) ||
        (strcmp(argv[4], "hold-parent") && strcmp(argv[4], "release-parent")) ||
        prctl(PR_SET_CHILD_SUBREAPER, 1) || pipe(gate) || pipe(info) || pipe(result)) return 2;
    parent = fork();
    if (parent < 0) return 3;
    if (!parent) {
        close(info[0]); close(result[0]);
        actor = fork();
        if (actor < 0) _exit(3);
        if (!actor) {
            close(info[1]); close(result[1]); close(gate[1]);
            if (read(gate[0], &go, 1) != 1) _exit(4);
            close(gate[0]);
            if (setgid(65534) || setuid(65534)) _exit(5);
            _exit(operation(argv[1], argv[2], argv[3]));
        }
        close(gate[0]); close(gate[1]);
        if (write(info[1], &actor, sizeof(actor)) != sizeof(actor)) _exit(7);
        close(info[1]);
        /* AS kills this live parent; the outer subreaper alone then reaps the actor. */
        if (!strcmp(argv[4], "hold-parent")) for (;;) pause();
        if (waitpid(actor, &actor_status, 0) != actor) _exit(8);
        if (write(result[1], &actor_status, sizeof(actor_status)) != sizeof(actor_status)) _exit(9);
        close(result[1]); _exit(0);
    }
    close(gate[0]); close(info[1]); close(result[1]);
    if (read(info[0], &actor, sizeof(actor)) != sizeof(actor)) return 10;
    close(info[0]);
    printf("{\"phase\":\"ready\",\"actor\":%ld,\"parent\":%ld}\n", (long)actor, (long)parent);
    fflush(stdout);
    if (read(STDIN_FILENO, &go, 1) != 1 || write(gate[1], &go, 1) != 1) return 11;
    close(gate[1]);
    if (waitpid(parent, &parent_status, 0) != parent) return 12;
    /* A surviving parent reports waitpid; otherwise subreaper owns the original actor. */
    ssize_t bytes = read(result[0], &actor_status, sizeof(actor_status));
    if (bytes == 0) { if (waitpid(actor, &actor_status, 0) != actor) return 13; }
    else if (bytes != sizeof(actor_status)) return 14;
    close(result[0]);
    printf("{\"actor\":%ld,\"parent\":%ld,\"actor_signal\":%d,\"parent_signal\":%d,\"actor_exit\":%d,\"parent_exit\":%d}\n",
           (long)actor, (long)parent,
           WIFSIGNALED(actor_status) ? WTERMSIG(actor_status) : 0,
           WIFSIGNALED(parent_status) ? WTERMSIG(parent_status) : 0,
           WIFEXITED(actor_status) ? WEXITSTATUS(actor_status) : -1,
           WIFEXITED(parent_status) ? WEXITSTATUS(parent_status) : -1);
    return 0;
}
