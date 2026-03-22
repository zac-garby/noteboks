; Standard markdown-inline highlights (from Zed's bundled markdown-inline)

(emphasis) @emphasis.markup

(strong_emphasis) @emphasis.strong.markup

(code_span) @text.literal.markup

(strikethrough) @strikethrough.markup

[
  (inline_link)
  (shortcut_link)
  (collapsed_reference_link)
  (full_reference_link)
  (image)
  (link_text)
  (link_label)
] @link_text.markup

(inline_link
  [
    "("
    ")"
  ] @link_uri.markup)

(image
  [
    "("
    ")"
  ] @link_uri.markup)

[
  (link_destination)
  (uri_autolink)
  (email_autolink)
] @link_uri.markup

; Wiki-links [[target]] — highlight the whole link and its destination

(wiki_link) @link_text.markup

(wiki_link
  (link_destination) @link_uri.markup)

; Hashtags — #foo #my-tag

(tag) @link_text.markup

; Pandoc-style citations — @citekey

(citation) @property.markup

; LaTeX math — $...$ and $$...$$

(latex_block) @text.literal.markup
(latex_block (latex_span_delimiter) @punctuation.delimiter.markup)
