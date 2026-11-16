// AniMathIO adaptations for Typst 0.15 and our embedded STIX Two Math font.
#scope.insert("diff", math.partial)
#scope.insert("textmath", it => text(font: "STIX Two Math", it))
// MiTeX emits each cases row as one alignment-containing content value.
// Replace its alignment point with a quad so the two columns cannot touch.
#scope.insert("cases", (..rows) => {
  let spaced(it) = {
    if it == $&$.body { return it + h(1em) }
    if it.has("children") { return it.children.map(spaced).sum(default: []) }
    it
  }
  math.cases(..rows.pos().map(spaced))
})
