// from https://typst.app/universe/package/codelst
#import "@preview/codelst:2.0.2": sourcecode
#import "@preview/cetz:0.3.2"
#import "function.typ": alert

#sourcecode[```typ
#cetz.canvas({
    import cetz.draw: *
    circle((0, 0))
    line((0, 0), (2, 1))
})
```]

#cetz.canvas({
    import cetz.draw: *
    circle((0, 0))
    line((0, 0), (2, 1))
})

#figure(
  image("./images/typst.png", width: 60pt),
  caption: [
    Typst logo
  ],
)
#alert[Problem]
