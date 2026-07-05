# cpp-lsp is completely useless right now

## perl5
### toke.c

- line 148 of toke.c gives NO references to the macro, even tho it has many uses
  - macros look like they aren't treated like actual symbols, while semantically they are
    of sorts, at least enum-ey ones like that

- gd does nothing on #include lines
  - even worse - if you gd on the .h in the quotes, it takes you to line 12173, where
    ther's a random var named `h`
  - includes should be treated like perl imports in their UX


### op.c

- `op_p->` by itself on a line gives no smart completion (falls back to global); it's
  possible that it's a syntax error the sentinel doesn't fix?
    - `*op_p->` DOES give completion
    - so does `op_p. == 5` w/ your cursor on the dot
  - the "you need to peel" diagnostic is not firing
- on line 185, no element of the OP enum is offered as a completion

- gd on OP (the type), after the deterministic fix, it now goes to a random macro; reachability has to get fixeded
  - there might be some element where you can catch the undef to see which macros don't
    get "exported"

- op_p->op_next - the op_next hovers as OP, when it's really OP*
- enum variants hover as their value definition (good) but show nothing about their type (bad)
  - in opnames.h there's spurious inlay hints again - of course every value is an opcode
  - also asymmetry; the enum variants show no references other than their own def


### op.h

- now, op_type on line 55 has issues (even tho we indeed gd to it properly from a usage site)
  - it has a unecessary inline hint (it's LITERALLY typed right there)
  - it does not show any references
    asymmetric)

## fmt

### src/format.cc

- the outline is - COMPLETELY USELESS, it shows a handful of random variables which are
  arguments to templates (lines 19 and 21)

### include/fmt/format.h

- line 1161 - gr returns no referenses to the struct, even tho it seems to be used a few
  times w/in several lines
  - this is asymmetric - gd on line 1167 does jump back

- macros that are clearly markers (define w/ no expansion) show up in the outline
