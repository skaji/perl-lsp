package My::Comp::Extra;

# Override reached only through load_own_components' current-package prefix.
sub save { return 3 }

1;
