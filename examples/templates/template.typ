#import typst_as_lib: input

#set page(paper: "a4")
#set text(font: "TeX Gyre Cursor", 11pt)

#let content = input.v
#let last_index = content.len() - 1

#for (i, elem) in content.enumerate() [
  == #elem.heading
  Text: #elem.text \
  Num1: #elem.num1 \
  Num2: #elem.num2 \
  #if elem.image != none [#image.decode(elem.image, height: 40pt)]
  #if i < last_index [
    #pagebreak()
  ]
]
