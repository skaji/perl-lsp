package PanelUser;

use Panel;

sub go {
  my $p = Shared::Panel->new;
  $p->panel_render;
}

1;
