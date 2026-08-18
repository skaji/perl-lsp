package Gold::Dual;
use strict;
use warnings;

# The app-tier provider of Gold::Dual.
#
# This file and its vendored twin declare the SAME package and live at the
# same relative path under two different @INC roots. The gold harness
# normalizes a location's path to its BASENAME, which is `Dual.pm` for both
# by construction — so the two are told apart by LINE, and the layouts here
# are deliberately different. Do not "tidy" the spacing.

sub new { bless {}, shift }

sub which_provider { 'app' }

sub app_only_marker { 'app' }

1;
