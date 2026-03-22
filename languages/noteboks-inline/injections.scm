; Inject LaTeX into $...$ and $$...$$ math spans

((latex_block) @injection.content
  (#set! injection.language "latex")
  (#set! injection.include-children true))
