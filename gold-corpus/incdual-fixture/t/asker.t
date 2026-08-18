use strict;
use warnings;
use lib '../incdual-vendor';
use Gold::Dual;

# `use lib` puts the vendored root FIRST for THIS file, so the same module
# name means a different file here than it does in lib/App.pm.
my $d = Gold::Dual->new;
print $d->which_provider, "\n";
