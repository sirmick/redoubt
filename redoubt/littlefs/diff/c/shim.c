// A narrow C interface to the littlefs reference, so the Rust side of the differential
// tests needs only a handful of FFI declarations with plain arguments. Host-only test code.
//
// The block device is a RAM image owned by the Rust caller.

#include <stdlib.h>
#include <string.h>
#include "lfs.h"

typedef struct shim {
    struct lfs_config cfg;
    lfs_t lfs;
    uint8_t *image;
} shim;

static int bd_read(const struct lfs_config *c, lfs_block_t block, lfs_off_t off, void *buffer, lfs_size_t size) {
    shim *s = c->context;
    memcpy(buffer, s->image + (size_t)block * c->block_size + off, size);
    return 0;
}

static int bd_prog(const struct lfs_config *c, lfs_block_t block, lfs_off_t off, const void *buffer, lfs_size_t size) {
    shim *s = c->context;
    memcpy(s->image + (size_t)block * c->block_size + off, buffer, size);
    return 0;
}

static int bd_erase(const struct lfs_config *c, lfs_block_t block) {
    shim *s = c->context;
    memset(s->image + (size_t)block * c->block_size, 0xff, c->block_size);
    return 0;
}

static int bd_sync(const struct lfs_config *c) {
    (void)c;
    return 0;
}

shim *shim_new(uint8_t *image, uint32_t block_size, uint32_t block_count, uint32_t prog_size,
        int32_t block_cycles, uint32_t disk_version) {
    shim *s = calloc(1, sizeof(shim));
    s->image = image;
    s->cfg.context = s;
    s->cfg.read = bd_read;
    s->cfg.prog = bd_prog;
    s->cfg.erase = bd_erase;
    s->cfg.sync = bd_sync;
    s->cfg.read_size = 1;
    s->cfg.prog_size = prog_size;
    s->cfg.block_size = block_size;
    s->cfg.block_count = block_count;
    s->cfg.block_cycles = block_cycles;
    s->cfg.cache_size = block_size;
    s->cfg.lookahead_size = 32;
    s->cfg.disk_version = disk_version;
    return s;
}

void shim_free(shim *s) { free(s); }

int shim_format(shim *s) { return lfs_format(&s->lfs, &s->cfg); }
int shim_mount(shim *s) { return lfs_mount(&s->lfs, &s->cfg); }
int shim_unmount(shim *s) { return lfs_unmount(&s->lfs); }
int shim_mkdir(shim *s, const char *path) { return lfs_mkdir(&s->lfs, path); }
int shim_remove(shim *s, const char *path) { return lfs_remove(&s->lfs, path); }
int shim_rename(shim *s, const char *from, const char *to) { return lfs_rename(&s->lfs, from, to); }
int shim_mkconsistent(shim *s) { return lfs_fs_mkconsistent(&s->lfs); }

int shim_setattr(shim *s, const char *path, uint8_t type, const void *data, uint32_t len) {
    return lfs_setattr(&s->lfs, path, type, data, len);
}

int shim_removeattr(shim *s, const char *path, uint8_t type) { return lfs_removeattr(&s->lfs, path, type); }

// Returns the attribute's length, or a negative error.
int shim_getattr(shim *s, const char *path, uint8_t type, void *buf, uint32_t cap) {
    return lfs_getattr(&s->lfs, path, type, buf, cap);
}

// Writes `len` bytes at `at` (after truncating to zero if `trunc`), then truncates to
// `cut` unless it is negative.
int shim_write(shim *s, const char *path, int create, int trunc, uint32_t at, const void *data, uint32_t len,
        int64_t cut) {
    lfs_file_t f;
    int flags = LFS_O_RDWR | (create ? LFS_O_CREAT : 0) | (trunc ? LFS_O_TRUNC : 0);
    int err = lfs_file_open(&s->lfs, &f, path, flags);
    if (err) return err;
    lfs_soff_t pos = lfs_file_seek(&s->lfs, &f, at, LFS_SEEK_SET);
    if (pos < 0) { lfs_file_close(&s->lfs, &f); return pos; }
    lfs_ssize_t n = lfs_file_write(&s->lfs, &f, data, len);
    if (n < 0) { lfs_file_close(&s->lfs, &f); return n; }
    if (cut >= 0) {
        err = lfs_file_truncate(&s->lfs, &f, (lfs_off_t)cut);
        if (err) { lfs_file_close(&s->lfs, &f); return err; }
    }
    return lfs_file_close(&s->lfs, &f);
}

// Reads a whole file into `buf`; returns its size (which may exceed `cap`) or an error.
int shim_read(shim *s, const char *path, void *buf, uint32_t cap) {
    lfs_file_t f;
    int err = lfs_file_open(&s->lfs, &f, path, LFS_O_RDONLY);
    if (err) return err;
    lfs_soff_t size = lfs_file_size(&s->lfs, &f);
    lfs_ssize_t n = lfs_file_read(&s->lfs, &f, buf, cap);
    lfs_file_close(&s->lfs, &f);
    return n < 0 ? n : size;
}

// Lists a directory as records: type (1 byte), size (4 bytes LE), name length (2 bytes LE),
// name. Returns the bytes used, or an error. "." and ".." are left out.
int shim_list(shim *s, const char *path, uint8_t *out, uint32_t cap) {
    lfs_dir_t d;
    struct lfs_info info;
    int err = lfs_dir_open(&s->lfs, &d, path);
    if (err) return err;
    uint32_t used = 0;
    int res;
    while ((res = lfs_dir_read(&s->lfs, &d, &info)) > 0) {
        if (!strcmp(info.name, ".") || !strcmp(info.name, "..")) continue;
        uint32_t nlen = strlen(info.name);
        if (used + 7 + nlen > cap) { lfs_dir_close(&s->lfs, &d); return LFS_ERR_NOSPC; }
        out[used] = (uint8_t)info.type;
        memcpy(out + used + 1, &info.size, 4);
        out[used + 5] = nlen & 0xff;
        out[used + 6] = nlen >> 8;
        memcpy(out + used + 7, info.name, nlen);
        used += 7 + nlen;
    }
    lfs_dir_close(&s->lfs, &d);
    return res < 0 ? res : (int)used;
}
