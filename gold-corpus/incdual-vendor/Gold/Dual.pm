# The vendored provider of Gold::Dual — the twin of incdual-app's copy.
#
# Same package, same relative path, a different @INC root. See the note in
# incdual-app/Gold/Dual.pm: the harness basenames a location's path, so the
# two copies are told apart by LINE NUMBER alone. The layouts are
# deliberately unequal — the `package` line and `which_provider` both sit
# where the app copy's do not. Do not align them.
package Gold::Dual;
use strict;
use warnings;

sub new { bless {}, shift }

sub vendor_only_marker { 'vendor' }

sub which_provider { 'vendor' }

1;
