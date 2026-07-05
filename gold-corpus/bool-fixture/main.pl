# Bool-typed expressions (InferredType::Bool across the lattice).
my $x = 3;
my $y = 4;

my $cmp = $x == $y;     # numeric comparison -> Bool
my $seq = $x <=> $y;    # ordering (-1/0/1)  -> Numeric
my $neg = !$x;          # logical negation   -> Bool
my $def = defined $x;   # truth-test builtin -> Bool
