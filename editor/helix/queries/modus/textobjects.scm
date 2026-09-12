(function_declaration) @function.around
(function_declaration
  body: (block) @function.inside)

(parameter) @parameter.inside
(parameter) @parameter.around

(argument_list
  (_) @parameter.inside)

(line_comment) @comment.around
(block_comment) @comment.around
