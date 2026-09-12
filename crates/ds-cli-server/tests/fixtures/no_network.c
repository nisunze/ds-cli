/*
 * The cut network this proof runs under.
 *
 * A Server that is offline-first must be provable offline, and "there is no
 * gateway in this test" is only as strong as the thing that would notice if
 * one appeared. This shim removes the possibility: loaded with LD_PRELOAD it
 * fails EVERY connect() to a non-loopback address with ENETUNREACH and EVERY
 * name lookup that is not a loopback name with EAI_FAIL, for the process and
 * for every process it spawns -- including the real `ds server serve` the
 * proof starts.
 *
 * Loopback stays open, because the proof's own listeners are loopback, and so
 * is every non-IP family (AF_UNIX and the kernel's own sockets), because
 * cutting those cuts the machine rather than the network.
 *
 * Built by `fixtures::cut_network()` with `cc -shared -fPIC`. It is C because
 * the interposition it performs is a property of the dynamic linker, and
 * because a compiler for it is on every machine that can build this workspace.
 */

#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>

/* Is this address one the proof's own machine answers on? */
static int ds_loopback(const struct sockaddr *address, socklen_t length) {
  if (address == NULL) {
    return 1;
  }
  if (address->sa_family == AF_INET && length >= sizeof(struct sockaddr_in)) {
    const struct sockaddr_in *v4 = (const struct sockaddr_in *)address;
    return (ntohl(v4->sin_addr.s_addr) >> 24) == 127;
  }
  if (address->sa_family == AF_INET6 && length >= sizeof(struct sockaddr_in6)) {
    const struct sockaddr_in6 *v6 = (const struct sockaddr_in6 *)address;
    if (IN6_IS_ADDR_LOOPBACK(&v6->sin6_addr)) {
      return 1;
    }
    if (IN6_IS_ADDR_V4MAPPED(&v6->sin6_addr)) {
      return v6->sin6_addr.s6_addr[12] == 127;
    }
    return 0;
  }
  /* AF_UNIX, AF_NETLINK and friends are the machine, not the network. */
  return 1;
}

int connect(int fd, const struct sockaddr *address, socklen_t length) {
  static int (*real)(int, const struct sockaddr *, socklen_t) = NULL;
  if (real == NULL) {
    real = dlsym(RTLD_NEXT, "connect");
  }
  if (!ds_loopback(address, length)) {
    errno = ENETUNREACH;
    return -1;
  }
  return real(fd, address, length);
}

static int ds_local_name(const char *node) {
  return node == NULL || strcmp(node, "localhost") == 0 ||
         strcmp(node, "localhost.localdomain") == 0 ||
         strcmp(node, "ip6-localhost") == 0 || strcmp(node, "127.0.0.1") == 0 ||
         strcmp(node, "::1") == 0;
}

int getaddrinfo(const char *node, const char *service,
                const struct addrinfo *hints, struct addrinfo **result) {
  static int (*real)(const char *, const char *, const struct addrinfo *,
                     struct addrinfo **) = NULL;
  if (real == NULL) {
    real = dlsym(RTLD_NEXT, "getaddrinfo");
  }
  if (!ds_local_name(node)) {
    return EAI_FAIL;
  }
  return real(node, service, hints, result);
}

struct hostent *gethostbyname(const char *name) {
  static struct hostent *(*real)(const char *) = NULL;
  if (real == NULL) {
    real = dlsym(RTLD_NEXT, "gethostbyname");
  }
  if (!ds_local_name(name)) {
    h_errno = HOST_NOT_FOUND;
    return NULL;
  }
  return real(name);
}
