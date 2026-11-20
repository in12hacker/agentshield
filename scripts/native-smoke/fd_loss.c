#define _GNU_SOURCE
/* Isolated guest: inherited device FD must not keep a dead controller alive. */
#include "as_protocol.h"
#include <endian.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

struct connection { int fd; uint8_t session[16]; uint64_t request; struct as_status status; unsigned direct; };
struct loader { pid_t pid; int input; };
struct report { pid_t holder, loader; };
static void fatal(const char *what) { perror(what); exit(1); }
static void check(int condition, const char *what) { if (!condition) { fprintf(stderr,"native fd-loss: %s\n",what); exit(1); } }
static void transfer(int fd, void *buffer, size_t bytes, int writing)
{
    size_t offset = 0;
    while (offset < bytes) {
        ssize_t count = writing ? write(fd,(char *)buffer+offset,bytes-offset) : read(fd,(char *)buffer+offset,bytes-offset);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) fatal(writing ? "write pipe" : "read pipe");
        offset += (size_t)count;
    }
}
static void ready(int fd, short events)
{
    struct pollfd p = { .fd=fd,.events=events };
    int result;
    do result=poll(&p,1,-1); while (result<0 && errno==EINTR);
    check(result==1 && !(p.revents&(POLLERR|POLLNVAL)),"native I/O did not become ready");
}
static size_t exchange(struct connection *c, uint16_t type, const void *body, size_t bytes, uint8_t *reply)
{
    uint8_t frame[AS_FRAME_MAX];
    struct as_frame header = { .magic=htole32(AS_MAGIC),.format=htole16(AS_FORMAT),.class=htole16(AS_CLASS_REQUEST),
        .type=htole16(type),.body_bytes=htole32((uint32_t)bytes),.request_id=htole64(++c->request),.total_bytes=htole64(bytes) };
    uint64_t request=c->request;
    ssize_t count;
    check(bytes <= sizeof(frame)-sizeof(header),"fixture frame too large");
    memcpy(header.session,c->session,16);
    memcpy(frame,&header,sizeof(header));
    if(bytes) memcpy(frame+sizeof(header),body,bytes);
    for (;;) {
        count=write(c->fd,frame,sizeof(header)+bytes);
        if(count<0 && errno==EINTR) continue;
        if(count<0 && errno==EAGAIN) { ready(c->fd,POLLOUT); continue; }
        check(count==(ssize_t)(sizeof(header)+bytes),"native write was not one complete frame");
        break;
    }
    for (;;) {
        count=read(c->fd,frame,sizeof(frame));
        if(count<0 && errno==EINTR) continue;
        if(count<0 && errno==EAGAIN) { ready(c->fd,POLLIN); continue; }
        check(count>=(ssize_t)(sizeof(header)+sizeof(struct as_reply)),"native read incomplete");
        memcpy(&header,frame,sizeof(header));
        check(le32toh(header.magic)==AS_MAGIC && le16toh(header.format)==AS_FORMAT &&
            !header.reserved16 && !header.reserved64 && le32toh(header.body_bytes)==(uint32_t)count-sizeof(header),"invalid current frame");
        if(le16toh(header.class)==AS_CLASS_EVENT) {
            if(le16toh(header.type)==AS_DIRECT)c->direct++;
            printf("{\"case\":\"native_fd_event\",\"type\":%u,\"frame_hex\":\"",le16toh(header.type));
            for(ssize_t i=0;i<count;i++)printf("%02x",frame[i]);
            puts("\"}");fflush(stdout);
            continue;
        }
        check(le16toh(header.class)==AS_CLASS_REPLY && le16toh(header.type)==type && le64toh(header.request_id)==request,"wrong native reply");
        struct as_reply status;
        memcpy(&status,frame+sizeof(header),sizeof(status));
        if((int32_t)le32toh(status.error)) fprintf(stderr,"native command %u errno %d\n",type,(int32_t)le32toh(status.error));
        check(!status.error && !status.reserved,"native command rejected");
        memcpy(c->session,header.session,16);
        bytes=(size_t)count-sizeof(header)-sizeof(status);
        memcpy(reply,frame+sizeof(header)+sizeof(status),bytes);
        return bytes;
    }
}
static void connect_native(struct connection *c)
{
    uint8_t body[AS_FRAME_MAX];
    memset(c,0,sizeof(*c));
    c->fd=open("/dev/as_wait",O_RDWR|O_NONBLOCK|O_CLOEXEC);
    if(c->fd<0) fatal("open native endpoint");
    check(exchange(c,AS_HELLO,NULL,0,body)==sizeof(struct as_status)+sizeof(struct as_capabilities),"Hello layout");
    memcpy(&c->status,body,sizeof(c->status));
    check(le32toh(c->status.self_protection)!=1,"diagnostic cannot replace active formal self protection");
}
static struct loader load_hooks(const char *suite, const uint8_t instance[16])
{
    int input[2], output[2];
    char executable[PATH_MAX], object[PATH_MAX], id[33], line[13];
    struct loader loader;
    for(unsigned i=0;i<16;i++) snprintf(id+i*2,3,"%02x",instance[i]);
    check(snprintf(executable,sizeof(executable),"%s/driver/as-bpf-loader",suite)<(int)sizeof(executable),"loader path");
    check(snprintf(object,sizeof(object),"%s/driver/as_hooks.bpf.o",suite)<(int)sizeof(object),"object path");
    if(pipe2(input,O_CLOEXEC)||pipe2(output,O_CLOEXEC)) fatal("loader pipes");
    loader.pid=fork();
    if(loader.pid<0) fatal("fork loader");
    if(!loader.pid) {
        if(dup2(input[0],0)<0||dup2(output[1],1)<0) _exit(121);
        close(input[0]);close(input[1]);close(output[0]);close(output[1]);
        execl(executable,executable,object,id,(char *)NULL);_exit(122);
    }
    close(input[0]);close(output[1]);loader.input=input[1];
    transfer(output[0],line,sizeof(line),0);close(output[0]);
    check(!memcmp(line,"AS-BPF-READY\n",sizeof(line)),"loader did not attach complete trusted set");
    return loader;
}
static void initialize(struct connection *c, const char *image)
{
    uint8_t bytes[AS_FRAME_MAX],reply[AS_FRAME_MAX];
    int fd=open(image,O_RDONLY|O_CLOEXEC);
    ssize_t count;
    if(fd<0) fatal("open fixture images");
    count=read(fd,bytes,sizeof(bytes));close(fd);
    check(count>=(ssize_t)sizeof(struct as_images) && count<(ssize_t)sizeof(bytes)-64,"fixture images length");
    struct as_images *images=(void *)bytes;
    images->expected_generation=c->status.generation;
    images->expected_epoch=c->status.lifecycle_epoch;
    check(exchange(c,AS_INITIALIZE,bytes,(size_t)count,reply)==sizeof(c->status),"Initialize status layout");
    memcpy(&c->status,reply,sizeof(c->status));
    check(le32toh(c->status.state)==1,"Initialize did not actually enter Running");
}
static void wait_success(pid_t pid, const char *what)
{
    int status;
    if(waitpid(pid,&status,0)!=pid) fatal(what);
    check(WIFEXITED(status)&&!WEXITSTATUS(status),what);
}
static void denied_open(const char *suite)
{
    char subject[PATH_MAX], path[PATH_MAX];
    check(snprintf(subject,sizeof(subject),"%s/subject",suite)<(int)sizeof(subject),"subject path");
    check(snprintf(path,sizeof(path),"%s/files/deny",suite)<(int)sizeof(path),"deny path");
    pid_t child=fork();if(child<0)fatal("fork direct subject");
    if(!child){execl(subject,subject,"open",path,(char *)NULL);_exit(122);}
    wait_success(child,"direct subject exit");
}
int main(int argc, char **argv)
{
    int report_pipe[2],status;
    struct report report;
    struct stat held,device;
    char held_path[128],failure[PATH_MAX],image[PATH_MAX];
    pid_t controller;
    if(argc!=2) return 2;
    check(prctl(PR_SET_CHILD_SUBREAPER,1)==0,"subreaper unavailable");
    check(snprintf(image,sizeof(image),"%s/diagnostic-images.bin",argv[1])<(int)sizeof(image),"image path");
    check(snprintf(failure,sizeof(failure),"%s/files/failure",argv[1])<(int)sizeof(failure),"failure path");
    if(pipe2(report_pipe,O_CLOEXEC)) fatal("report pipe");
    controller=fork();if(controller<0) fatal("fork controller");
    if(!controller) {
        struct connection c;
        int held_ready[2];
        close(report_pipe[0]);connect_native(&c);
        struct loader loader=load_hooks(argv[1],c.status.instance);
        initialize(&c,image);
        uint8_t tracking[AS_FRAME_MAX];
        check(exchange(&c,AS_START_FACTS,NULL,0,tracking)==sizeof(struct as_facts_marker),"start real facts before controller death");
        denied_open(argv[1]);
        if(pipe2(held_ready,O_CLOEXEC)) fatal("inherited FD ready pipe");
        report.holder=fork();if(report.holder<0) fatal("fork inherited FD holder");
        if(!report.holder) {
            int native_copy=fcntl(c.fd,F_DUPFD_CLOEXEC,20);
            int ready_copy=fcntl(held_ready[1],F_DUPFD_CLOEXEC,20);
            if(native_copy<0||ready_copy<0||dup2(native_copy,3)<0||dup2(ready_copy,4)<0) _exit(123);
            /* In particular close the inherited loader stdin writer. */
            if(close_range(5,UINT_MAX,0)) _exit(124);
            close(0);close(1);close(2);
            char r='R';transfer(4,&r,1,1);close(4);
            for(;;) pause();
        }
        close(held_ready[1]);char r;transfer(held_ready[0],&r,1,0);close(held_ready[0]);
        check(r=='R',"holder readiness");report.loader=loader.pid;
        transfer(report_pipe[1],&report,sizeof(report),1);close(report_pipe[1]);
        for(;;) pause();
    }
    close(report_pipe[1]);transfer(report_pipe[0],&report,sizeof(report),0);close(report_pipe[0]);
    check(kill(controller,SIGKILL)==0,"kill actual controller");
    check(waitpid(controller,&status,0)==controller && WIFSIGNALED(status) && WTERMSIG(status)==SIGKILL,"actual controller exit");
    wait_success(report.loader,"loader did not exit after actual parent death");
    check(snprintf(held_path,sizeof(held_path),"/proc/%ld/fd/3",(long)report.holder)<(int)sizeof(held_path),"holder proc path");
    check(!stat(held_path,&held)&&!stat("/dev/as_wait",&device)&&S_ISCHR(held.st_mode)&&held.st_rdev==device.st_rdev,"fork child did not retain native FD");
    errno=0;int fd=open(failure,O_RDONLY|O_CLOEXEC);int error=errno;if(fd>=0)close(fd);
    printf("{\"case\":\"controller_death_inherited_fd\",\"controller\":%ld,\"holder\":%ld,\"loader\":%ld,\"loader_exited\":true,\"native_fd_inherited\":true,\"open_result\":%d,\"errno\":%d}\n",(long)controller,(long)report.holder,(long)report.loader,fd>=0?0:-1,error);fflush(stdout);
    check(fd<0&&error==EIO,"dead controller did not enforce Failed while child retained FD");
    struct connection recovered;connect_native(&recovered);
    struct loader loader=load_hooks(argv[1],recovered.status.instance);
    initialize(&recovered,image);
    /* Old inherited-file release must not erase the newly authenticated controller. */
    check(kill(report.holder,SIGKILL)==0,"kill old-session FD holder after recovery");
    check(waitpid(report.holder,&status,0)==report.holder&&WIFSIGNALED(status),"actual old holder exit");
    uint8_t reply[AS_FRAME_MAX];
    check(exchange(&recovered,AS_STATUS,NULL,0,reply)==sizeof(struct as_status),"recovered status after old FD release");
    struct as_status live;memcpy(&live,reply,sizeof(live));
    check(le32toh(live.state)==1&&live.generation==recovered.status.generation,"old FD release erased recovered Running session");
    puts("{\"case\":\"old_fd_release_after_recovery\",\"result\":\"pass\",\"state\":\"running\"}");
    check(exchange(&recovered,AS_START_FACTS,NULL,0,reply)==sizeof(struct as_facts_marker),"start recovered facts");
    unsigned before=recovered.direct;
    denied_open(argv[1]);
    check(exchange(&recovered,AS_QUIESCE,NULL,0,reply)==sizeof(struct as_status),"recovery Quiesce status");
    struct as_status stopped;memcpy(&stopped,reply,sizeof(stopped));
    check(le32toh(stopped.state)==2&&!stopped.pending&&!stopped.receipts,"recovery did not actually drain");
    unsigned drained=recovered.direct-before;
    denied_open(argv[1]);
    struct pollfd tail={.fd=recovered.fd,.events=POLLIN};
    int readable=poll(&tail,1,0);if(readable<0)fatal("inspect stopped observation tail");
    printf("{\"case\":\"direct_quiesce_tail\",\"direct_before_reply\":%u,\"readable_after_stopped_operation\":%s}\n",drained,readable?"true":"false");fflush(stdout);
    /* Match DriverSession shutdown: release the drained control connection
     * before retiring this instance's BPF links. */
    close(recovered.fd);
    char command='U';transfer(loader.input,&command,1,1);close(loader.input);
    wait_success(loader.pid,"recovery loader retirement");
    puts("{\"case\":\"explicit_recovery_and_drain\",\"result\":\"pass\",\"status_polling\":false}");
    /* Classify after actual cleanup; never interrupt an advancing operation. */
    return drained==1&&!readable?0:1;
}
