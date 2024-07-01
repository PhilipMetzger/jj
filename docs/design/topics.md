# Topics (virtual topological branches and metadata)

Authors: [Philip Metzger](mailto:philipmetzger@bluewin.ch), [Noah Mayr](mailto:dev@noahmayr.com)
 [Anton Bulakh](mailto:him@necaq.ua)

## Summary

Introduce Topics as a truly Jujutsu native way for topological branches, which
replace the current bookmark concept for Git interop. As they have been
documented to be confusing users coming from Git. They also supersede the
`[experimental-advance-branches]` config for those who currently use it, as
such a behavior will be built-in for Topics.

Topics have been discussed heavily since their appearance in
[this Discussion][gh-discuss]. As Noah, Anton and I had a long
[Discord discussion][dc-thread] about them, which then also poured into the
[Topic issue][issue].

## Prior work

Currently there only is Mercurial which has a implementation of
[Topics][hg-topics]. There also is the [Topic feature][gerrit-topics] in Gerrit,
which groups commits with a single identifier. Also the Heptapod Forge, a Fork
of Gitlab supports [Mercurial Topics][heptapod-topics] as an Git branch 
equivalent, which shares some similarity to this proposal. Heptapod although 
imposes multiple restrictions on Topics, such as only supporting a single head
since their underlying Git server doesn't map these Topics to multiple Git 
branches. 

## Goals and non-goals

### Goals

The goals for this Project are small, see below.

* Introduce the concept of native topological branches for Jujutsu.
* Simplify Git interop by reducing the burden on Bookmarks.
* Add Change metadata as a storage concept.
* Remove the awkward `bookmark` to Git `branch` mapping.

### Non-Goals

* Making bookmarks as Tags obsolete. 
* Introduce something like Git's branching concept.
* Requiring Topic support for every existing backend.
* 

## Overview

Until now, Jujutsu had no native set of topological branches, just
[Bookmarks][bm] which interact poorly with Git's expectation of branches.
Topics on the otherhand are can be made to represent Git branches as typical 
non-expert Git users expect them, see [Julia Evans poll][jvns-poll]. With a 
large adjustment to the [`tracking branches`][tracking] model, they can act as
the primary point for Git-interopability building on existing Bookmark 
functionality. A subset of the Bookmark machinery is still required, as 
the `trunk()` revset and other internals depend on it. 

This frees up the current Bookmark concept to function as a `git tag` 
equivalent, which fits better for a Jujutsu native future.

Other use-cases they're useful for are representing a set of
[archived commits][archived] or even a [checkout history][checkout].



#### Behavior

A Topic is a set of changes marked with a name, which infectiously moves
forward with each descendant you create. All changes without a named topic are
implicitly in a anonymous topic, which gets separately tracked as soon as you
send it in for review or materialize it as a Git branch. A Topic may have
multiple heads. A single Change may belong to multiple Topics, which means
that they also can get exported in overlapping Git branches.

TODO: Example here

The simplest example for Topics is the solo-developer who only cares about 
advancing the `main` Topic or Git branch:

(Topic are denoted as '<name> in the following diagrams)

```text
@
|
C 'main
|
B 'main
|
A 'main
```
TODO: Add Excalidraw image here

A `jj new` on the current working copy will mark the parent as well as the 
new working copy commit on the `main` Topic, which then can get pushed to the 
users favorite Git remote as the main branch, removing the need for a `jj tug`
alias which advances the Bookmark.

**BatmanAod's case**:

This one is from the longer conversation in Discord, where it a contributor 
has a workflow where they continuously branch off `main` or another custom 
bookmark and repeatedly send these to the CI system, even if the work mainly
is exploratory or experimental.

```text
@
| \ \
E  E' E ('experiment)
|  | /
D  D' ('feature)
| / 
C
|
B
|
A 'main
```
TODO: Add Excalidraw image here


#### Major Use-Cases

##### Obsoletion of `[experimental.auto-advance-bookmarks]`

Since Topics are infectious by nature, they can perfectly map to a Git branch.
This alleviates the need for the auto-advancing bookmarks. It also makes the
use-case of the solo developer working on Git `main` easier, as you just mark
all descendants of `trunk()` belonging to the `main` topic, which then gets
translated to the `main` branch on `jj git push`.


##### Change Archival

For the archival use-case the infectious property of a Topic isn't as
important. So having a
`jj topic create -r 'root()..@ & description("archive:")' should mark them all
revisions with a `archive:` description as archived. For this use-case the
non-contiguous propertie of Topics really shine.

##### Checkout history

Topics also align for the checkout history use-case, as a non-contiguous Topic
is perfectly able to tag every `edit`ed or `new`ed commit.

### Detailed Design

#### Topics and `trunk()`

While a Topic may track `trunk()`, `trunk()` always must be a revset as a 
dependant definition leads to infinite recursion i.e a undecidable statement.

#### Git Interop

For all continuous Topics we can just simply export them as Git Branches. For
Topics which contain non-contiguous parts, we should allow exporting them
one-by-one or with a generated name, such as `<topic-name>-1` for each part.
We also could disallow exporting such Topics as Git branches which is similar 
to our policy with conflicted commits. 

#### Internals

A Topic is defined as two or more non-contiguous refs and the commits within
their either full or partial ranges. These refs are determined by keeping a 
internal bookmark on the [start, end] and all individual commits on the topic.
(TODO: This may be expensive). Since Topics are pure metadata on a single 
commit there's no need for a special `rebase` command. 


##### Command interactions and new flags 

###### `jj new`

`jj new` will gain a bunch of new flags which determine if the parents Topic 
should follow or not. By default it will move the parents topics to the new 
revision. On revisions with no associated Topics

###### `jj rebase`

Since Topics are metadata, rebasing certain revisions will make Topics disjoint
and may break a implicit unnamed Topic. This means rebase will also need some 
flag which optionally discards all Topic metadata. 

##### `jj duplicate`

Since topics stick to a single revision, `jj duplicate` will copy them to the 
new copy. For the use-case of moving the copy to either a new topic or directly
removing it will need two grow two new flags. 

##### Configuration options

Topics can be configured by revsets, such as `subject(glob:archive*)` which 
marks all visible commit which contain the configured subject as belonging to
the aforementioned topic. There was an idea to support non-infectious topics 
as named revisions by making the behavior configurable, but as the behavior 
already overlaps with actual Jujutsu bookmarks it was discarded.

#### Storage

We should store `Topics` as metadata on the serialized proto, without
considering the resulting Gencode. To prevent needless commit rewrites the 
metadata must be ignored for commit hash rewrites, since it was a problem 
in Noahs PoC. 


```protobuf
// A simple Key-Value pair.
message StringPair {
  string key = 1;
  string value = 2;
  // Could be extended by a protobuf.Any see the future possibilities section.
}

message Commit {
  //...
  repeated StringPair metadata = N;
}
```

while the actual code should look like this:

```rust
#[derive(ContentHash, ...)]
struct Commit {
  //...
  //
  // This avoids rewriting the Change-ID, but must be implemented.
  #[ContentHash(ignore)]
  topics: HashMap<String, String>
}
```

the Rust type is a hashmap to allow topics to be namespaced. To truly conform 
to the protobuf, it should be a btree_map instead. Exluding metadata from 
the content hash, makes it feel lightweight. 

#### Backend implications

If Topics were stored as commit metadata, it would allow backends to drop
them if necessary. This property can be useful to mark tests as passing
on a specific client or avoiding a field entirely in database backed backends.
To make Topic lookup fast they also should be indexed on the `View` since 
travering many revisions has a performance impact in Repositories such as the 
Google monorepo. 

For the Git backend, we could either embed them in the message, like Arcanist
or Gerrit do or store them as Git Notes, if necessary.

## Alternatives considered

### Local Topics

See [Noah's prototype][prototype] for the variant of keeping them out of band.
While this works it falls short of having the metadata synced by multiple
clients, which is something desirable. The prototype thus also avoids rewriting
the Change-ID which is a good thing, but makes them only locally available. 
While it is not the chosen design it will be built upon to match the proposed 
Topics. It is the baseline for all further work. 


### Single Head Topics

While these are conceptually simpler, they wouldn't help with Git interop where
it is useful to map a single underlying Topic to multiple Git branches. This
also worsens the `jj`-`Git` interop story.

## Future Possibilities

In the future we could attach a `google.protobuf.Any` to the Change metadata,
which would allow specific clients, such as testrunners to directly attach test
results to a Change which could be neat. We also could export `jj`-native 
Topics as Gerrit Topics.

The commit metadata could be used as formally correct application of 
[commit trailers].

[archived]: https://github.com/martinvonz/jj/discussions/4180
[bm]:  ../bookmarks.md
[checkout]: https://github.com/martinvonz/jj/issues/3713
[dc-thread]: https://discord.com/channels/968932220549103686/1224085912464527502
[gerrit-topics]: https://gerrit-review.googlesource.com/Documentation/cross-repository-changes.html
[gh-discuss]: https://github.com/martinvonz/jj/discussions/2425#discussioncomment-7376935
[hg-topics]: https://www.mercurial-scm.org/doc/evolution/tutorials/topic-tutorial.html#topic-basics
[issue]: https://github.com/martinvonz/jj/discussions/2425#discussioncomment-7376935
[jvns-poll]: https://social.jvns.ca/@b0rk/111709458396281239
[prototype]: https://github.com/martinvonz/jj/pull/3613
