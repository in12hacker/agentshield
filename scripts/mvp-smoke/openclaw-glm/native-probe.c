#include <arpa/inet.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    static const char marker[] = "AS native operation completed\n";
    if (argc != 3)
        return 2;
    if (strcmp(argv[1], "mark") == 0) {
        FILE *file = fopen(argv[2], "wb");
        if (file == NULL)
            return 2;
        size_t written = fwrite(marker, 1, sizeof(marker) - 1, file);
        int closed = fclose(file);
        if (written != sizeof(marker) - 1 || closed != 0)
            return 2;
        printf("{\"family\":\"execute\",\"pid\":%ld,\"written\":%zu}\n", (long)getpid(), written);
        return 0;
    }

    int sending = strcmp(argv[1], "send") == 0;
    if (!sending && strcmp(argv[1], "connect") != 0)
        return 2;
    char *end;
    long port = strtol(argv[2], &end, 10);
    if (*end || port < 1 || port > 65535)
        return 2;
    struct sockaddr_in peer = {
        .sin_family = AF_INET,
        .sin_addr.s_addr = htonl(INADDR_LOOPBACK),
        .sin_port = htons((unsigned short)port),
    };
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0)
        return 2;
    ssize_t result = connect(fd, (struct sockaddr *)&peer, sizeof(peer));
    int error = result < 0 ? errno : 0;
    int connected = result == 0;
    if (sending && connected) {
        result = send(fd, marker, sizeof(marker) - 1, 0);
        error = result < 0 ? errno : 0;
    }
    close(fd);
    printf("{\"family\":\"%s\",\"pid\":%ld,\"port\":%ld,\"connected\":%d,"
           "\"native_return\":%zd,\"errno\":%d}\n",
           argv[1], (long)getpid(), port, connected, result, error);
    return error != 0;
}
