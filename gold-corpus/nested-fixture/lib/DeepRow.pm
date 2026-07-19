package DeepRow;
use strict;
use warnings;

# TWO hops to DBIx::Class: DeepRow -> DeepBase (separate file) ->
# DBIx::Class::Core. The build-time trigger sees only the local parent
# `DeepBase`, so column/relationship synthesis is DEFERRED and re-fired at
# enrichment / index completion once the cross-file ancestry resolves.
use parent 'DeepBase';

__PACKAGE__->add_columns(
    title => { data_type => 'varchar' },
    body  => { data_type => 'text' },
);

__PACKAGE__->has_many( widgets => 'Widget', 'row_id' );

1;
