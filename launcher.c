/*
 * launcher.c — YAAGL macOS .app bundle launcher
 * Replaces the shell script "parameterized" as CFBundleExecutable.
 * Compiles to a universal Mach-O binary with no runtime dependencies.
 *
 * Build:
 *   clang -Os -arch arm64 -arch x86_64 \
 *         -mmacosx-version-min=10.15 \
 *         -o launcher launcher.c && strip launcher
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <limits.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <mach-o/dyld.h>

static int strip_components(char *path, int n) {
    for (int i = 0; i < n; i++) {
        char *slash = strrchr(path, '/');
        if (!slash) return -1;
        *slash = '\0';
    }
    return 0;
}

static int mkdirp(const char *path) {
    char tmp[PATH_MAX];
    snprintf(tmp, sizeof(tmp), "%s", path);
    size_t len = strlen(tmp);
    if (len && tmp[len-1] == '/') tmp[len-1] = '\0';
    for (char *p = tmp + 1; *p; p++) {
        if (*p == '/') {
            *p = '\0';
            if (mkdir(tmp, 0755) != 0 && errno != EEXIST) return -1;
            *p = '/';
        }
    }
    if (mkdir(tmp, 0755) != 0 && errno != EEXIST) return -1;
    return 0;
}

int main(void) {
    /* 1. Resolve own path */
    char exe_path[PATH_MAX];
    uint32_t size = (uint32_t)sizeof(exe_path);
    if (_NSGetExecutablePath(exe_path, &size) != 0) {
        fprintf(stderr, "launcher: _NSGetExecutablePath failed\n");
        return 1;
    }
    char real_exe[PATH_MAX];
    if (!realpath(exe_path, real_exe)) {
        fprintf(stderr, "launcher: realpath failed: %s\n", strerror(errno));
        return 1;
    }

    /* 2. Derive MacOS dir and Contents dir */
    char macos_dir[PATH_MAX];
    char contents_dir[PATH_MAX];
    snprintf(macos_dir,    sizeof(macos_dir),    "%s", real_exe);
    snprintf(contents_dir, sizeof(contents_dir), "%s", real_exe);
    if (strip_components(macos_dir, 1) != 0 ||
        strip_components(contents_dir, 2) != 0) {
        fprintf(stderr, "launcher: cannot derive bundle paths\n");
        return 1;
    }

    /* 3. Get app name from CFBundleName via env or use MacOS dir parent name */
    /* We read the app support dir name from the binary name for generality */
    const char *home = getenv("HOME");
    if (!home || !*home) {
        fprintf(stderr, "launcher: $HOME not set\n");
        return 1;
    }

    /* App support dir name = binary name next to us (the Yaagl/Yaagl binary) */
    /* We find the actual NeutralinoJS binary: first non-self executable in MacOS */
    /* Simplest: binary is always called "Yaagl" — read from a known sibling */
    char yaagl_bin[PATH_MAX];
    snprintf(yaagl_bin, sizeof(yaagl_bin), "%s/Yaagl", macos_dir);

    /* Derive app support name from parent .app name */
    char app_bundle[PATH_MAX];
    snprintf(app_bundle, sizeof(app_bundle), "%s", contents_dir);
    strip_components(app_bundle, 1); /* now points to .app dir */
    char *app_dir_name = strrchr(app_bundle, '/');
    if (!app_dir_name) {
        fprintf(stderr, "launcher: cannot get app bundle name\n");
        return 1;
    }
    app_dir_name++; /* skip leading '/' */
    /* strip .app suffix */
    char app_name[PATH_MAX];
    snprintf(app_name, sizeof(app_name), "%s", app_dir_name);
    size_t alen = strlen(app_name);
    if (alen > 4 && strcmp(app_name + alen - 4, ".app") == 0)
        app_name[alen - 4] = '\0';

    /* 4. Build ~/Library/Application Support/<AppName> */
    char apst_dir[PATH_MAX];
    snprintf(apst_dir, sizeof(apst_dir),
             "%s/Library/Application Support/%s", home, app_name);
    if (mkdirp(apst_dir) != 0) {
        fprintf(stderr, "launcher: mkdirp(%s) failed: %s\n", apst_dir, strerror(errno));
        return 1;
    }

    /* 5. rsync Resources → Application Support */
    char resources_src[PATH_MAX];
    snprintf(resources_src, sizeof(resources_src), "%s/Resources/.", contents_dir);
    {
        pid_t pid = fork();
        if (pid < 0) {
            fprintf(stderr, "launcher: fork failed: %s\n", strerror(errno));
            return 1;
        }
        if (pid == 0) {
            char *rsync_argv[] = { "rsync", "-rlptu", resources_src, apst_dir, NULL };
            execvp("rsync", rsync_argv);
            fprintf(stderr, "launcher: execvp(rsync) failed: %s\n", strerror(errno));
            _exit(1);
        }
        int status = 0;
        while (waitpid(pid, &status, 0) == -1)
            if (errno != EINTR) break;
    }

    /* 6. chdir and exec the real binary */
    if (chdir(apst_dir) != 0) {
        fprintf(stderr, "launcher: chdir(%s) failed: %s\n", apst_dir, strerror(errno));
        return 1;
    }
    char path_arg[PATH_MAX + 8];
    snprintf(path_arg, sizeof(path_arg), "--path=%s", apst_dir);
    char *yaagl_argv[] = { yaagl_bin, path_arg, NULL };
    execv(yaagl_bin, yaagl_argv);

    fprintf(stderr, "launcher: execv(%s) failed: %s\n", yaagl_bin, strerror(errno));
    return 1;
}
