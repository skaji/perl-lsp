package My::Comp;
use base 'My::Base';

# DBIC-style own-namespace mixin loader: pulls in My::Comp::Extra.
__PACKAGE__->load_own_components('Extra');

1;
