;; comments
(comment) @comment
(shebang) @comment
((directive) @meta)

;; literals
(integer)   @number
(float)     @number
(imaginary) @number
(string)        @string
(raw_string)    @string
(format_string) @string
(tag)   @tag
(true)  @boolean
(false) @boolean
(inf)   @constant.builtin

;; identifiers
(variable_ref (identifier) @variable)
(identifier) @variable
(outer_variable (identifier) @variable.outer)

;; control flow
["$" "$." "$$" "W" "N" "B" "A" "and" "O" "or"] @keyword

(return_form) @keyword.return
(break_form) @keyword
(continue_form) @keyword
(try_form) @keyword
(debug_form) @keyword
(pause_form) @keyword
(symbolic_form) @keyword
"@i" @keyword
(depth_modifier) @operator

;; operators
["+:"
 "-:"
 "*:"
 "/:"
 "/.:"
 "%:"
 "^:"
 "^.:"
 ",:"
 "/%:"
 "+"
 "-"
 "*"
 "**"
 "/"
 "/."
 "/%"
 "%"
 "^"
 "^."
 "="
 "=."
 "~"
 "~."
 "<"
 "<="
 ">"
 ">="
 "#"
 ".."
 "..="] @operator

(ellipsis) @operator
(operator_identifier) @operator
(pipe_operator) @operator.pipe

;; punctuation
[":" "," ";"] @punctuation.delimiter
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
