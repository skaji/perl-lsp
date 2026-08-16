package User;

use Alpha;
use Beta;

sub go {
  Shared::Thing::from_alpha();
  Shared::Thing::from_beta();
  Alpha::Second::only_in_second();
}

1;
