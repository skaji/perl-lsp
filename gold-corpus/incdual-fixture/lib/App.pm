package App;
use strict;
use warnings;
use Gold::Dual;

# No `use lib`: this asker sees only the process @INC, so Gold::Dual is
# the app-tier provider.
sub run {
    my $d = Gold::Dual->new;
    return $d->which_provider;
}

1;
