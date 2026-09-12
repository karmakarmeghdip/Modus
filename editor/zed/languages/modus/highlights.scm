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
  name: (identifier) @variant)

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
  name: (identifier) @property)

(record_type_field
  name: (identifier) @property)

(record_pattern_field
  name: (identifier) @property)

(field_expression
  field: (identifier) @property)

; Pattern Variants
(variant_pattern
  type: (identifier) @type)

(variant_pattern
  variant: (identifier) @variant)

; Literals
(string_literal) @string
(escape_sequence) @string.escape
(integer_literal) @number
(float_literal) @number
(boolean_literal) @boolean

; Comments
(line_comment) @comment
(block_comment) @comment

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
