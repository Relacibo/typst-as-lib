#import sys: inputs

#let content = inputs.v
#let last_index = content.len() - 1

#for (i, elem) in content.enumerate() [
  == #elem.heading
  Text: #elem.text \
  Num1: #elem.num1 \
  Num2: #elem.num2 \
  // Doesn't work yet?
  #if elem.image != none [#image.decode(elem.image, height: 40pt)]
  #if i < last_index [
    #html.elem("hr")
  ]
]
