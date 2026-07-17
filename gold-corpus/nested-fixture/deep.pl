use strict;
use warnings;
use lib 'lib';
use DeepRow;

# The invocant is directly typed via the constructor pattern (DeepRow->new),
# so goto-def / references on the 2-hop-synthesized accessors must reach
# DeepRow.pm — proving cross-file ClassIsa synthesis is visible cross-file.
my $row = DeepRow->new;
my $w   = $row->widgets;
my $t   = $row->title;
