#!/usr/bin/env perl
# Derive a cpanfile for a Perl APPLICATION that ships no manifest.
#
# Most large open-source Perl applications are distributed as packages, not CPAN
# dists: of the eight corpus repos, six declare their dependencies nowhere a tool
# can read. Their real dependency set is what they `use`, minus what they define,
# minus what ships with perl. That is derivable, and deriving it beats a
# hand-curated list that rots.
#
# Deliberately over-includes: a name we cannot classify is emitted, because a
# spurious requirement costs one failed install line while a missing one silently
# leaves the import unresolvable — and an unresolvable import costs the resolver
# NOTHING to answer, which is exactly the bias that makes a corpus measure the
# wrong thing.
use strict;
use warnings;
use File::Find;
use Module::CoreList;

my $root = shift // '.';
my %used, my %defined;

find(sub {
    return unless -f && /\.(pm|pl|t|cgi)$/;
    open my $fh, '<', $_ or return;
    while (my $line = <$fh>) {
        next if $line =~ /^\s*#/;
        # package declarations = what this repo PROVIDES
        $defined{$1} = 1 if $line =~ /^\s*package\s+([A-Za-z_][\w:]*)/;
        # use/require of a bareword module = what it CONSUMES
        if ($line =~ /^\s*(?:use|require)\s+([A-Za-z_][\w:]*)/) {
            my $m = $1;
            $used{$m} = 1;
        }
    }
}, $root);

# Pragmas and feature-ish bareword targets that are not distributions.
my %pragma = map { $_ => 1 } qw(
    strict warnings utf8 lib vars constant parent base overload feature
    integer bytes locale open sort subs bigint bignum bigrat encoding
    if less mro re sigtrap version fields attributes autouse blib
    diagnostics filetest inc iso8859 charnames deprecate experimental
);

my @want;
for my $m (sort keys %used) {
    next if $pragma{$m};
    next if $defined{$m};                      # provided in-repo
    next if $m =~ /^[a-z]/ && $m !~ /::/;      # lowercase bareword: pragma-shaped
    # core for the running perl? then no install needed.
    my $first = Module::CoreList->first_release($m);
    next if defined $first && Module::CoreList->is_core($m, undef, $]);
    push @want, $m;
}

print "# Derived by corpus/derive-cpanfile.pl — see corpus/README.md\n";
print "# Over-inclusive on purpose: a missing dep silently makes an import\n";
print "# unresolvable, and unresolvable imports cost the resolver nothing,\n";
print "# which biases every measurement taken against this corpus.\n";
printf "requires '%s';\n", $_ for @want;
printf STDERR "%s: %d used, %d in-repo, %d to install\n",
    $root, scalar(keys %used), scalar(keys %defined), scalar(@want);
