package WidgetUser;

use Widget;

sub go {
  my $w = Shared::Widget->new;
  $w->extra_method;
  $w->base_method;
}

1;
