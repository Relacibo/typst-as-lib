// from https://typst.app/universe/package/codelst
#import "@preview/codelst:2.0.1": sourcecode
#import "function.typ": alert

#sourcecode[```typ
#show "ArtosFlow": name => box[
  #box(image(
    "logo.svg",
    height: 0.7em,
  ))
  #name
]

This report is embedded in the
ArtosFlow project. ArtosFlow is a
project of the Artos Institute.
```]

#figure(
  image("./images/typst.png", width: 60pt),
  caption: [
    Typst logo
  ],
)
#alert[Problem]
