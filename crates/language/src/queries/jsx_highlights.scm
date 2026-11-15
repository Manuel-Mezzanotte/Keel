; JSX highlighting for TSX files.
;
; Keel puts these rules in front of gpui-component's TypeScript rules, so that
; tag and attribute names win over the generic identifier rules that follow.

(jsx_opening_element name: (identifier) @tag)
(jsx_closing_element name: (identifier) @tag)
(jsx_self_closing_element name: (identifier) @tag)

(jsx_opening_element name: (member_expression) @tag)
(jsx_closing_element name: (member_expression) @tag)
(jsx_self_closing_element name: (member_expression) @tag)

(jsx_attribute (property_identifier) @attribute)

(jsx_opening_element ["<" ">"] @punctuation.bracket)
(jsx_closing_element ["</" ">"] @punctuation.bracket)
(jsx_self_closing_element ["<" "/>"] @punctuation.bracket)

(jsx_expression ["{" "}"] @punctuation.special)
