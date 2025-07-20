---- MODULE cluster_fault_tolerance ----
EXTENDS Naturals, Sequences

CONSTANTS Nodes
VARIABLES State, Log, Leader

(* A node state is either Follower, Candidate or Leader *)
Follower == 0
Candidate == 1
LeaderRole == 2

TypeInvariant == /\ Leader \in Nodes \cup {None}
                /\ Log \in [Nodes -> Seq( Nat )]
                /\ State \in [Nodes -> {Follower, Candidate, LeaderRole}]

LeaderUnique == \A n1, n2 \in Nodes: (State[n1] = LeaderRole /\ State[n2] = LeaderRole) => n1 = n2

Safety == LeaderUnique

==== 