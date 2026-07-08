wq-cli's internal editor

forked from `rustyline`, itself based on `linenoise`

See [NOTICE.md](NOTICE.md) for upstream attribution and license notices.

**Supported Platforms**

- Unix (tested on FreeBSD, Linux and macOS)
- Windows
  - cmd.exe
  - Powershell

**Note**:

- Powershell ISE is not supported, check [issue #56](https://github.com/kkawakam/rustyline/issues/56)
- Mintty (Cygwin/MinGW) is not supported
- Highlighting / Colors are not supported on Windows < Windows 10 except with ConEmu and `ColorMode::Forced`.

## Actions

For all modes:

| Keystroke             | Action                                                                      |
| --------------------- | --------------------------------------------------------------------------- |
| Home                  | Move cursor to the beginning of line                                        |
| End                   | Move cursor to end of line                                                  |
| Left                  | Move cursor one character left                                              |
| Right                 | Move cursor one character right                                             |
| Ctrl-C                | Interrupt/Cancel edition                                                    |
| Ctrl-D, Del           | (if line is _not_ empty) Delete character under cursor                      |
| Ctrl-D                | (if line _is_ empty) End of File                                            |
| Ctrl-J, Ctrl-M, Enter | Finish the line entry                                                       |
| Ctrl-R                | Reverse Search history (Ctrl-S forward, Ctrl-G cancel)                      |
| Ctrl-T                | Transpose previous character with current character                         |
| Ctrl-U                | Delete from start of line to cursor                                         |
| Ctrl-V (unix)         | Insert any special character without performing its associated action (#65) |
| Ctrl-V (windows)      | Paste from clipboard                                                        |
| Ctrl-W                | Delete word leading up to cursor (using white space as a word boundary)     |
| Ctrl-Y                | Paste from Yank buffer                                                      |
| Ctrl-Z                | Suspend (Unix only)                                                         |
| Ctrl-\_               | Undo                                                                        |

### Emacs mode (default mode)

| Keystroke         | Action                                                                                           |
| ----------------- | ------------------------------------------------------------------------------------------------ |
| Ctrl-A, Home      | Move cursor to the beginning of line                                                             |
| Ctrl-B, Left      | Move cursor one character left                                                                   |
| Ctrl-E, End       | Move cursor to end of line                                                                       |
| Ctrl-F, Right     | Move cursor one character right (or complete hint if cursor is at the end of line)               |
| Ctrl-H, Backspace | Delete character before cursor                                                                   |
| Shift-Tab         | Previous completion                                                                              |
| Ctrl-I, Tab       | Next completion                                                                                  |
| Ctrl-K            | Delete from cursor to end of line                                                                |
| Ctrl-L            | Clear screen                                                                                     |
| Ctrl-N, Down      | Next match from history                                                                          |
| Ctrl-P, Up        | Previous match from history                                                                      |
| Ctrl-X Ctrl-G     | Abort                                                                                            |
| Ctrl-X Esc        | Abort                                                                                            |
| Ctrl-X Ctrl-U     | Undo                                                                                             |
| Ctrl-X Backspace  | Delete from cursor to the beginning of line                                                      |
| Ctrl-Y            | Paste from Yank buffer (Meta-Y to paste next yank instead)                                       |
| Ctrl-] <char>     | Search character forward                                                                         |
| Ctrl-Alt-] <char> | Search character backward                                                                        |
| Meta-<            | Move to first entry in history                                                                   |
| Meta->            | Move to last entry in history                                                                    |
| Meta-B, Alt-Left  | Move cursor to previous word                                                                     |
| Ctrl-Left         | See Alt-Left                                                                                     |
| Meta-C            | Capitalize the current word                                                                      |
| Meta-D            | Delete forwards one word                                                                         |
| Meta-F, Alt-Right | Move cursor to next word                                                                         |
| Ctrl-Right        | See Alt-Right                                                                                    |
| Meta-L            | Lower-case the next word                                                                         |
| Meta-T            | Transpose words                                                                                  |
| Meta-U            | Upper-case the next word                                                                         |
| Meta-Y            | See Ctrl-Y                                                                                       |
| Meta-Backspace    | Kill from the start of the current word, or, if between words, to the start of the previous word |
| Meta-0, 1, ..., - | Specify the digit to the argument. `–` starts a negative argument.                               |

[Readline Emacs Editing Mode Cheat Sheet](http://www.catonmat.net/download/readline-emacs-editing-mode-cheat-sheet.pdf)

### vi command mode

| Keystroke            | Action                                                                      |
| -------------------- | --------------------------------------------------------------------------- |
| $, End               | Move cursor to end of line                                                  |
| .                    | Redo the last text modification                                             |
| ;                    | Redo the last character finding command                                     |
| ,                    | Redo the last character finding command in opposite direction               |
| 0, Home              | Move cursor to the beginning of line                                        |
| ^                    | Move to the first non-blank character of line                               |
| a                    | Insert after cursor                                                         |
| A                    | Insert at the end of line                                                   |
| b                    | Move one word or token left                                                 |
| B                    | Move one non-blank word left                                                |
| c<movement>          | Change text of a movement command                                           |
| C                    | Change text to the end of line (equivalent to c$)                           |
| d<movement>          | Delete text of a movement command                                           |
| D, Ctrl-K            | Delete to the end of the line                                               |
| e                    | Move to the end of the current word                                         |
| E                    | Move to the end of the current non-blank word                               |
| f<char>              | Move right to the next occurrence of `char`                                 |
| F<char>              | Move left to the previous occurrence of `char`                              |
| h, Ctrl-H, Backspace | Move one character left                                                     |
| l, Space             | Move one character right                                                    |
| Ctrl-L               | Clear screen                                                                |
| i                    | Insert before cursor                                                        |
| I                    | Insert at the beginning of line                                             |
| +, j, Ctrl-N         | Move forward one command in history                                         |
| -, k, Ctrl-P         | Move backward one command in history                                        |
| p                    | Insert the yanked text at the cursor (paste)                                |
| P                    | Insert the yanked text before the cursor                                    |
| r                    | Replaces a single character under the cursor (without leaving command mode) |
| R                    | Replaces a single character under the cursor (entering the replace mode)    |
| s                    | Delete a single character under the cursor and enter input mode             |
| S                    | Change current line (equivalent to 0c$)                                     |
| t<char>              | Move right to the next occurrence of `char`, then one char backward         |
| T<char>              | Move left to the previous occurrence of `char`, then one char forward       |
| u                    | Undo                                                                        |
| w                    | Move one word or token right                                                |
| W                    | Move one non-blank word right                                               |
| x                    | Delete a single character under the cursor                                  |
| X                    | Delete a character before the cursor                                        |
| y<movement>          | Yank a movement into buffer (copy)                                          |
| <<movement>          | Dedent                                                                      |
| ><movement>          | Indent                                                                      |

### vi insert mode

| Keystroke         | Action                                        |
| ----------------- | --------------------------------------------- |
| Ctrl-H, Backspace | Delete character before cursor                |
| Shift-Tab         | Previous completion                           |
| Ctrl-I, Tab       | Next completion                               |
| Right             | Complete hint if cursor is at the end of line |
| Alt-<char>        | Fast command mode                             |
| Esc               | Switch to command mode                        |

[Readline vi Editing Mode Cheat Sheet](http://www.catonmat.net/download/bash-vi-editing-mode-cheat-sheet.pdf)

[ANSI escape code](https://en.wikipedia.org/wiki/ANSI_escape_code)

## Wine

```sh
$ cargo run --example example --target 'x86_64-pc-windows-gnu'
...
Error: Io(Error { repr: Os { code: 6, message: "Invalid handle." } })
$ wineconsole --backend=curses target/x86_64-pc-windows-gnu/debug/examples/example.exe
...
```

## Terminal checks

```sh
$ # current settings of all terminal attributes:
$ stty -a
$ # key bindings:
$ bind -p
$ # print out a terminfo description:
$ infocmp
```

## Multi line support

This is a very simple feature that simply causes lines that are longer
than the current terminal width to be displayed on the next visual
line instead of horizontally scrolling as more characters are
typed. Currently, this feature is always enabled and there is no
configuration option to disable it.

This feature does not allow the end user to hit a special key
sequence and enter a mode where hitting the return key will cause a
literal newline to be added to the input buffer.

The way to achieve multi-line editing is to implement the `Validator`
trait.

## Minimum supported Rust version (MSRV)

Latest stable Rust version at the time of release. It might compile with older versions.
