; Keywords
"function" @keyword.function
"return" @keyword.return

[
  "type"
  "let"
  "trait"
  "impl"
  "extern"
  "library"
  "import"
  "export"
  "from"
  "as"
  "for"
] @keyword

[
  "if"
  "else"
  "match"
] @keyword.control

[
  "perform"
  "check"
] @keyword.operator

; Types
(primitive_type) @type.builtin

(type_declaration
  name: (identifier) @type)

(trait_declaration
  name: (identifier) @type)

(impl_declaration
  trait: (identifier) @type)

(type_param
  name: (identifier) @type.parameter)

(variant_declaration
  name: (identifier) @constructor)

(generic_type
  name: (identifier) @type)

(path_type
  (identifier) @type)

(parameter
  type: (identifier) @type)

(record_type_field
  type: (identifier) @type)

; Functions
(function_declaration
  name: (identifier) @function)

(trait_member
  name: (identifier) @function)

(call_expression
  function: (identifier) @function.call)

(call_expression
  function: (field_expression
    field: (identifier) @function.method))

; Variables & Parameters
(parameter
  name: (identifier) @variable.parameter)

(let_statement
  name: (identifier) @variable)

(identifier) @variable

; Properties / Fields
(record_field
  name: (identifier) @variable.other.member)

(record_type_field
  name: (identifier) @variable.other.member)

(record_pattern_field
  name: (identifier) @variable.other.member)

(field_expression
  field: (identifier) @variable.other.member)

; Pattern Variants
(variant_pattern
  type: (identifier) @type)

(variant_pattern
  variant: (identifier) @constructor)

; Literals
(string_literal) @string
(escape_sequence) @constant.character.escape
(integer_literal) @constant.numeric
(float_literal) @constant.numeric
(boolean_literal) @constant.builtin.boolean

; Comments
(line_comment) @comment.line
(block_comment) @comment.block

; Operators
[
  "+"
  "-"
  "*"
  "/"
  "%"
  "=="
  "!="
  "<"
  "<="
  ">"
  ">="
  "&&"
  "||"
  "!"
  "=>"
  "="
  "..."
  "|"
] @operator

; Delimiters and Brackets
[
  ","
  ";"
  ":"
  "."
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket
