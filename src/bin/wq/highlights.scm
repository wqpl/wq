;; comments
(comment) @comment
(shebang) @comment
((magic_command) @meta)

;; literals
(integer)   @number
(float)     @number
(string)    @string
(raw_string) @string
(format_string) @string
(character) @character
(symbol_lit) @constant.builtin
(true)  @boolean
(false) @boolean
(inf)   @constant.builtin
(nan)   @constant.builtin

;; identifiers
(variable_ref (builtin) @function.builtin)
(variable_ref (identifier) @variable)
(builtin) @function.builtin
(identifier) @variable

;; control flow
((conditional "$"  @keyword))
((conditional_dot "$." @keyword))
((w_loop "W" @keyword))
((n_loop   "N" @keyword))

(return_form   "@r"  @keyword.return)
(break_form    "@b"   @keyword)
(continue_form "@c" @keyword)
(try_form      "@t"     @keyword)

;; operators
["+" "-" "*" "**" "/" "/." "%" "%." "^" "|"
 "=" "~" "<" "<=" ">" ">=" "#" ".." "..="] @operator
(ellipsis) @operator

;; punctuation
[":" "," ";"] @punctuation.delimiter
["(" ")" "[" "]" "{" "}"] @punctuation.bracket

;; Name binding when assigning a function literal: foo: { ... }
;; Capture the identifier anywhere within the left subtree when RHS has a function literal.
((assignment
   left: (_ (variable_ref (identifier) @function))) @assign
  (#has-descendant? @assign function_literal))

;; Fallback: highlight function literals themselves (body blocks)
(function_literal) @function

;; Calls: any postfix with at least one suffix
((postfix
   (postfix
     primary: (primary (variable_ref (identifier) @function.call)))
   (suffix)))
((postfix
   (postfix
     primary: (primary (variable_ref (builtin) @function.builtin)))
   (suffix)))

;; Call arguments
;; - Juxtaposition argument (f 1)
(suffix (juxtaposition_arg) @argument)
;; - Index arguments inside brackets (f[1], f[1;2], f[])
;;   Match any bracket item expression directly
(item (expression) @argument)

;; Pipe call highlighting: RHS function name in a pipe
;; Simple function on RHS: x | f
((pipe_expr
   (postfix
     primary: (primary (variable_ref (identifier) @function.call)))))

;; Function with one suffix on RHS: x | f[...]
((pipe_expr
   (postfix
     (postfix
       primary: (primary (variable_ref (identifier) @function.call))))))

;; Builtin variants on RHS of pipe
((pipe_expr
   (postfix
     primary: (primary (variable_ref (builtin) @function.builtin)))))
((pipe_expr
   (postfix
     (postfix
       primary: (primary (variable_ref (builtin) @function.builtin))))))
