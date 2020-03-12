#include <stdbool.h>
#include <sys/types.h>
#include <sys/wait.h>

struct exit_status {
    int value;
    bool terminated;
};

int wait_for_process(pid_t process_id, struct exit_status *exit_status) {
    siginfo_t process_info;

    int result = waitid(
        P_PID,
        process_id,
        &process_info,
        WEXITED | WNOWAIT | WSTOPPED
    );
    if (result < 0) {
        return result;
    }

    exit_status->value = process_info.si_status;
    exit_status->terminated = process_info.si_code != CLD_EXITED;

    return 0;
}
