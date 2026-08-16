package Shared::Widget;

sub new { my ($class) = @_; return bless {}, $class }

sub base_method { return 'base' }

1;
