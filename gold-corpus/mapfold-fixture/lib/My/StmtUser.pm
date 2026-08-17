package My::StmtUser;
use Moo;
with map { my $n = "Role::$_"; $n } qw(Alpha Beta);

sub run { return 1 }

1;
