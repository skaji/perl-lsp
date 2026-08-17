package Shared::Panel;

use parent -norequire, 'Shared::PanelBase';

sub hook { my ($self) = @_; return 'hook' }

1;
