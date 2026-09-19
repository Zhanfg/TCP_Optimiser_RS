// SPDX-License-Identifier: GPL-2.0
/*
 * Minimal userspace loader for the TCP Optimiser struct_ops probe.
 *
 * Research-only: it attaches tcpopt_probe, verifies that the congestion
 * controller becomes visible to TCP, then detaches before exiting.
 */
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <bpf/libbpf.h>

#define OPS_MAP_NAME "tcpopt_probe"
#define CC_LIST "/proc/sys/net/ipv4/tcp_available_congestion_control"

static int has_word(const char *haystack, const char *needle)
{
    const char *p = haystack;
    size_t n = strlen(needle);

    while ((p = strstr(p, needle)) != NULL) {
        int left_ok = p == haystack || p[-1] == ' ' || p[-1] == '\n' || p[-1] == '\t';
        char right = p[n];
        int right_ok = right == '\0' || right == ' ' || right == '\n' || right == '\t';
        if (left_ok && right_ok)
            return 1;
        p += n;
    }
    return 0;
}

static int congestion_control_visible(const char *name)
{
    FILE *file = fopen(CC_LIST, "re");
    char buf[4096];
    size_t len;

    if (!file)
        return -errno;
    len = fread(buf, 1, sizeof(buf) - 1, file);
    if (ferror(file)) {
        int err = errno ? errno : EIO;
        fclose(file);
        return -err;
    }
    fclose(file);
    buf[len] = '\0';
    return has_word(buf, name) ? 1 : 0;
}

int main(int argc, char **argv)
{
    struct bpf_object_open_opts open_opts = {};
    const char *path;
    struct bpf_object *obj = NULL;
    struct bpf_map *ops = NULL;
    struct bpf_link *link = NULL;
    int err;
    int visible;

    if (argc != 2) {
        fprintf(stderr, "usage: %s <tcpopt_probe.bpf.o>\n", argv[0]);
        return 2;
    }
    path = argv[1];

    open_opts.sz = sizeof(open_opts);
    obj = bpf_object__open_file(path, &open_opts);
    err = libbpf_get_error(obj);
    if (err) {
        fprintf(stderr, "open: %s\n", strerror(-err));
        return 1;
    }

    ops = bpf_object__find_map_by_name(obj, OPS_MAP_NAME);
    if (!ops) {
        fprintf(stderr, "map %s not found\n", OPS_MAP_NAME);
        bpf_object__close(obj);
        return 1;
    }

    err = bpf_object__load(obj);
    if (err) {
        fprintf(stderr, "load: %s (%d)\n", strerror(-err), err);
        bpf_object__close(obj);
        return 1;
    }

    link = bpf_map__attach_struct_ops(ops);
    err = libbpf_get_error(link);
    if (err) {
        fprintf(stderr, "attach: %s (%d)\n", strerror(-err), err);
        bpf_object__close(obj);
        return 1;
    }

    visible = congestion_control_visible(OPS_MAP_NAME);
    if (visible < 0) {
        fprintf(stderr, "verify: %s\n", strerror(-visible));
        err = visible;
    } else if (!visible) {
        fprintf(stderr, "%s registered but not visible in %s\n", OPS_MAP_NAME, CC_LIST);
        err = -ENOENT;
    } else {
        printf("struct_ops probe registered successfully: %s\n", OPS_MAP_NAME);
        err = 0;
    }

    bpf_link__destroy(link);
    bpf_object__close(obj);
    return err ? 1 : 0;
}
