package My::Base;

# The contract method — goto-implementation on this decl fans out to every
# class that overrides it in a concrete descendant's dispatch table.
sub save { return 1 }

1;
