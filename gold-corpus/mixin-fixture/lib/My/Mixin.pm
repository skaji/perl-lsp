package My::Mixin;

# A sibling-parent override: does NOT inherit My::Base, but is composed
# ALONGSIDE it into My::Child, so it wins/participates in Child's dispatch.
sub save { return 2 }

1;
