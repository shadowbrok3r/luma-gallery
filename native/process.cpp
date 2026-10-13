#include <jni.h>
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
Java_app_luma_gallery_MediaProcess_spawn(JNIEnv *env, jclass, jobjectArray arguments, jintArray inputs, jintArray targets) {
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
    std::vector<jint> from(env->GetArrayLength(inputs)), to(from.size());
    env->GetIntArrayRegion(inputs, 0, static_cast<jsize>(from.size()), from.data());
    env->GetIntArrayRegion(targets, 0, static_cast<jsize>(to.size()), to.data());
    int pipefd[2];
    int error = pipe2(pipefd, O_CLOEXEC) ? errno : 0;
    pid_t pid = -1;
    if (!error) {
        pid = fork();
        if (pid == 0) {
            dup2(pipefd[1], STDOUT_FILENO);
            dup2(pipefd[1], STDERR_FILENO);
            // Targets are stdin or above every input descriptor.
            bool input = false;
            for (size_t i = 0; i < from.size(); ++i) {
                dup2(from[i], to[i]);
                input = input || to[i] == STDIN_FILENO;
            }
            if (!input) {
                int null = open("/dev/null", O_RDONLY | O_CLOEXEC);
                if (null >= 0 && null != STDIN_FILENO) dup2(null, STDIN_FILENO);
            }
            execve(argv[0], argv.data(), environ);
            _exit(127);
        }
        error = pid < 0 ? errno : 0;
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
