package My::Requirer;

require Req::Target;

sub use_it {
    my $class = "Req::Target";
    require $class;
    return Req::Target::greet();
}

1;
