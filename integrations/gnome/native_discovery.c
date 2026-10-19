#define _GNU_SOURCE
#include "native_discovery.h"
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define CURRENT "lyn-gnome-v1.current"
#define SOCKET_NAME "lyn-gnome-v1.sock"
#define INSTANCE_BYTES 35
#define RECORD_BYTES (INSTANCE_BYTES + 1)

static bool private_node(const struct stat *node, mode_t type, mode_t permissions)
{
    return (node->st_mode & S_IFMT) == type && node->st_uid == geteuid()
        && (node->st_mode & 07777) == permissions
        && (type != S_IFREG || node->st_nlink == 1);
}

static bool same_inode(const struct stat *left, const struct stat *right)
{
    return left->st_dev == right->st_dev && left->st_ino == right->st_ino;
}

static bool same_record(const struct stat *left, const struct stat *right)
{
    return same_inode(left, right) && private_node(right, S_IFREG, 0600)
        && left->st_size == right->st_size
        && left->st_mtim.tv_sec == right->st_mtim.tv_sec
        && left->st_mtim.tv_nsec == right->st_mtim.tv_nsec
        && left->st_ctim.tv_sec == right->st_ctim.tv_sec
        && left->st_ctim.tv_nsec == right->st_ctim.tv_nsec;
}

static bool named_node(int parent, const char *name, const struct stat *expected,
                       mode_t type, mode_t permissions)
{
    struct stat actual;
    return fstatat(parent, name, &actual, AT_SYMLINK_NOFOLLOW) == 0
        && same_inode(expected, &actual) && private_node(&actual, type, permissions);
}

static bool instance_name(const char *record)
{
    if (memcmp(record, "gn-", 3) || record[INSTANCE_BYTES] != '\n') return false;
    for (size_t index = 3; index < INSTANCE_BYTES; index++) {
        unsigned char value = (unsigned char)record[index];
        if (!((value >= '0' && value <= '9') || (value >= 'a' && value <= 'f')))
            return false;
    }
    return true;
}

bool lyn_native_discover(const char *runtime, char endpoint[LYN_NATIVE_ENDPOINT_BYTES])
{
    if (!runtime || !endpoint || runtime[0] != '/') return false;
    char root[LYN_NATIVE_ENDPOINT_BYTES], result[LYN_NATIVE_ENDPOINT_BYTES];
    size_t length = strnlen(runtime, sizeof(root));
    if (length == sizeof(root)) return false;
    memcpy(root, runtime, length + 1);
    /* lstat/open with a trailing slash or /. would otherwise follow a final
     * directory symlink. Strip those spellings without resolving symlinks. */
    for (;;) {
        while (length > 1 && root[length - 1] == '/') length--;
        if (length >= 2 && root[length - 2] == '/' && root[length - 1] == '.') {
            length -= 2;
            if (!length) length = 1;
        } else break;
    }
    root[length] = '\0';
    if (length + 1 + INSTANCE_BYTES + 1 + strlen(SOCKET_NAME) >= sizeof(result))
        return false;

    int directory = -1, publication = -1, instance = -1;
    bool valid = false;
    struct stat root_node, record_node, instance_node, socket_node, final_record;
    directory = open(root, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK);
    if (directory < 0 || fstat(directory, &root_node) != 0
        || !private_node(&root_node, S_IFDIR, 0700)) goto done;
    publication = openat(directory, CURRENT, O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK);
    if (publication < 0 || fstat(publication, &record_node) != 0
        || !private_node(&record_node, S_IFREG, 0600)
        || record_node.st_size != RECORD_BYTES) goto done;
    char bytes[RECORD_BYTES + 1], basename[INSTANCE_BYTES + 1];
    /* One bounded read: short read/EINTR is unavailable, never an unbounded
     * GTK retry. FIFO/device/symlink nodes were refused before any read. */
    if (pread(publication, bytes, sizeof(bytes), 0) != RECORD_BYTES
        || !instance_name(bytes)) goto done;
    memcpy(basename, bytes, INSTANCE_BYTES);
    basename[INSTANCE_BYTES] = '\0';
    instance = openat(directory, basename, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK);
    if (instance < 0 || fstat(instance, &instance_node) != 0
        || !private_node(&instance_node, S_IFDIR, 0700)
        || fstatat(instance, SOCKET_NAME, &socket_node, AT_SYMLINK_NOFOLLOW) != 0
        || !private_node(&socket_node, S_IFSOCK, 0600)) goto done;
    int count = snprintf(result, sizeof(result), "%s/%s/%s", root, basename, SOCKET_NAME);
    if (count < 0 || (size_t)count >= sizeof(result)) goto done;
    /* Hold the directory/record descriptors throughout lookup and fence each
     * name again. This is a coherent address hint, not a peer lifetime pin;
     * the channel still checks kernel credentials after its actual connect. */
    if (fstat(publication, &final_record) != 0 || !same_record(&record_node, &final_record)
        || !named_node(AT_FDCWD, root, &root_node, S_IFDIR, 0700)
        || !named_node(directory, CURRENT, &record_node, S_IFREG, 0600)
        || !named_node(directory, basename, &instance_node, S_IFDIR, 0700)
        || !named_node(instance, SOCKET_NAME, &socket_node, S_IFSOCK, 0600)) goto done;
    valid = true;
done:
    if (instance >= 0) close(instance);
    if (publication >= 0) close(publication);
    if (directory >= 0) close(directory);
    if (valid) memcpy(endpoint, result, strlen(result) + 1);
    return valid;
}
