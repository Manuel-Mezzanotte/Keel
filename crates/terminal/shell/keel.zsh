# Keel's shell integration for zsh. It marks where each prompt starts,
# the command line, where its output starts and how it ended, with the
# sequences VS Code reads (OSC 133 and 633). Keel draws a dot by each
# command from them, and jumps from one to the next.
[[ -o interactive ]] || return 0
[[ -n $__keel_integrated ]] && return 0
typeset -g __keel_integrated=1
typeset -g __keel_nonce=$KEEL_NONCE
typeset -gi __keel_running=0
unset KEEL_NONCE

# The command line, with what would end the sequence escaped as VS Code
# does: a backslash doubled, `;` and control characters as \xNN.
__keel_escape() {
  REPLY=${1//\\/\\\\}
  REPLY=${REPLY//;/\\x3b}
  REPLY=${REPLY//$'\n'/\\x0a}
  REPLY=${REPLY//$'\r'/\\x0d}
  REPLY=${REPLY//$'\e'/\\x1b}
  REPLY=${REPLY//$'\a'/\\x07}
}

# First of the precmd hooks, while $? is still the command's.
__keel_command_end() {
  local code=$?
  if (( __keel_running )); then
    builtin print -n "\e]133;D;$code\a"
    __keel_running=0
  fi
}

# Last of the precmd hooks, right before the prompt.
__keel_prompt_start() {
  builtin print -n "\e]133;A\a"
}

__keel_command_start() {
  local REPLY
  __keel_escape "$1"
  builtin print -rn -- $'\e]633;E;'"$REPLY;$__keel_nonce"$'\a\e]133;C\a'
  __keel_running=1
}

precmd_functions=(__keel_command_end ${precmd_functions:#__keel_*} __keel_prompt_start)
preexec_functions+=(__keel_command_start)
