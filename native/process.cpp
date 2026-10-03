#include <jni.h>
#include <spawn.h>
#include <sys/wait.h>
#include <unistd.h>
#include <fcntl.h>
#include <cerrno>
#include <climits>
#include <cstring>
#include <string>
#include <vector>

extern char **environ;

extern "C" JNIEXPORT jintArray JNICALL
Java_app_luma_gallery_MediaProcess_spawn(JNIEnv *env, jclass, jobjectArray arguments, jint input) {
    std::vector<std::string> strings;
    for (int i = 0; i < env->GetArrayLength(arguments); ++i) {
        auto value = static_cast<jstring>(env->GetObjectArrayElement(arguments, i));
        const char *text = env->GetStringUTFChars(value, nullptr);
        strings.emplace_back(text);
        env->ReleaseStringUTFChars(value, text);
        env->DeleteLocalRef(value);
    }
    std::vector<char *> argv;
    for (auto &s : strings) argv.push_back(s.data());
    argv.push_back(nullptr);
    int pipefd[2];
    int error = pipe2(pipefd, O_CLOEXEC) ? errno : 0;
    pid_t pid = -1;
    if (!error) {
        posix_spawn_file_actions_t actions;
        posix_spawn_file_actions_init(&actions);
        if (input >= 0) posix_spawn_file_actions_adddup2(&actions, input, STDIN_FILENO);
        else posix_spawn_file_actions_addopen(&actions, STDIN_FILENO, "/dev/null", O_RDONLY, 0);
        posix_spawn_file_actions_adddup2(&actions, pipefd[1], STDOUT_FILENO);
        posix_spawn_file_actions_adddup2(&actions, pipefd[1], STDERR_FILENO);
        posix_spawn_file_actions_addclose(&actions, pipefd[0]);
        posix_spawn_file_actions_addclose(&actions, pipefd[1]);
        error = posix_spawn(&pid, argv[0], &actions, nullptr, argv.data(), environ);
        posix_spawn_file_actions_destroy(&actions);
        close(pipefd[1]);
        if (error) close(pipefd[0]);
    }
    if (error) {
        env->ThrowNew(env->FindClass("java/io/IOException"), strerror(error));
        return nullptr;
    }
    int result[] = {pid, pipefd[0]};
    auto array = env->NewIntArray(2);
    env->SetIntArrayRegion(array, 0, 2, result);
    return array;
}

extern "C" JNIEXPORT jint JNICALL
Java_app_luma_gallery_MediaProcess_waitChild(JNIEnv *, jclass, jint pid, jboolean block) {
    int status;
    pid_t result;
    do { result = waitpid(pid, &status, block ? 0 : WNOHANG); } while (result < 0 && errno == EINTR);
    if (!result) return INT_MIN;
    if (result < 0) return 255;
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
