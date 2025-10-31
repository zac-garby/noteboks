(section
  (headline (stars) @context (#match? @context "^(\\*{3})*\\*$") (item) @name)) @item

(section
  (headline (stars) @context (#match? @context "^(\\*{3})*\\*\\*$") (item) @name)) @item

(section
  (headline (stars) @context (#match? @context "^(\\*{3})*\\*\\*\\*$") (item) @name)) @item
