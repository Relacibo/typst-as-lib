// Automatic package bundling demo template
#import "@preview/cetz:0.3.2"

= Auto-Bundled Packages Demo

This template uses packages that are automatically detected and embedded at build time.

#cetz.canvas({
    import cetz.draw: *

    circle((0, 0), radius: 1, fill: blue.lighten(80%))
    line((0, 0), (2, 1), stroke: 2pt + red)
    content((1, 0.5), [Embedded!])
})

Packages are served directly from static data without filesystem access at runtime.
