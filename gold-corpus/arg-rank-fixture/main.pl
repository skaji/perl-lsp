package Foo;
sub apply {
    my ($count) = @_;
    $count = 5;
    return $count;
}
my $label = "x";
my $total = 3;
apply();
