package My::ExprUser;
use Moo;
with map "Role::$_", qw(Alpha Beta);

sub run { return 1 }

1;
