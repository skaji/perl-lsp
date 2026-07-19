package DeepBase;
use strict;
use warnings;

# Intermediate base: a DBIC result class reaches DBIx::Class through THIS
# file (a separate compilation unit), so the leaf's `ClassIsa("DBIx::Class")`
# trigger is only satisfiable CROSS-FILE. Mirrors DBICTest::BaseResult.
use parent 'DBIx::Class::Core';

1;
