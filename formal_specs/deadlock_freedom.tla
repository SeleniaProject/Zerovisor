---- MODULE deadlock_freedom ----
EXTENDS TLC, Integers, Sequences, FiniteSets

CONSTANTS
    Processes,     \* Set of processes
    Resources,     \* Set of resources
    MaxRequests    \* Maximum resource requests per process

VARIABLES
    holding,       \* Resources currently held by each process
    requesting,    \* Resources currently requested by each process
    available,     \* Available resources
    allocation     \* Resource allocation matrix

vars == <<holding, requesting, available, allocation>>

\* Resource allocation state
ResourceState == [Processes -> SUBSET Resources]

\* Deadlock detection using cycle detection in wait-for graph
HasDeadlock ==
    \E cycle \in Seq(Processes) :
        /\ Len(cycle) > 1
        /\ \A i \in 1..Len(cycle) :
            LET p1 == cycle[i]
                p2 == cycle[IF i = Len(cycle) THEN 1 ELSE i + 1]
            IN \E r \in Resources :
                /\ r \in requesting[p1]
                /\ r \in holding[p2]

\* Banker's algorithm safety check
SafeState ==
    \E sequence \in Seq(Processes) :
        /\ Len(sequence) = Cardinality(Processes)
        /\ \A i \in DOMAIN sequence : sequence[i] \in Processes
        /\ BankersCheck(sequence, available)

\* Banker's algorithm helper
BankersCheck(seq, avail) ==
    IF Len(seq) = 0 THEN TRUE
    ELSE
        LET p == Head(seq)
            need == requesting[p] \ holding[p]
        IN
            /\ need \subseteq avail
            /\ BankersCheck(Tail(seq), avail \cup holding[p])

\* Initial state - no deadlocks
Init ==
    /\ holding = [p \in Processes |-> {}]
    /\ requesting = [p \in Processes |-> {}]
    /\ available = Resources
    /\ allocation = [p \in Processes |-> [r \in Resources |-> 0]]

\* Request a resource
RequestResource(process, resource) ==
    /\ resource \in available
    /\ resource \notin holding[process]
    /\ Cardinality(requesting[process]) < MaxRequests
    /\ requesting' = [requesting EXCEPT ![process] = @ \cup {resource}]
    /\ UNCHANGED <<holding, available, allocation>>

\* Allocate a resource
AllocateResource(process, resource) ==
    /\ resource \in requesting[process]
    /\ resource \in available
    /\ LET new_holding == holding[process] \cup {resource}
           new_available == available \ {resource}
           new_requesting == requesting[process] \ {resource}
       IN
           /\ SafeStateAfterAllocation(process, resource)
           /\ holding' = [holding EXCEPT ![process] = new_holding]
           /\ available' = new_available
           /\ requesting' = [requesting EXCEPT ![process] = new_requesting]
           /\ allocation' = [allocation EXCEPT ![process][resource] = 1]

\* Release a resource
ReleaseResource(process, resource) ==
    /\ resource \in holding[process]
    /\ holding' = [holding EXCEPT ![process] = @ \ {resource}]
    /\ available' = available \cup {resource}
    /\ allocation' = [allocation EXCEPT ![process][resource] = 0]
    /\ UNCHANGED requesting

\* Safety check for allocation
SafeStateAfterAllocation(process, resource) ==
    LET new_holding == [holding EXCEPT ![process] = @ \cup {resource}]
        new_available == available \ {resource}
        new_requesting == [requesting EXCEPT ![process] = @ \ {resource}]
    IN
        \E sequence \in Seq(Processes) :
            /\ Len(sequence) = Cardinality(Processes)
            /\ BankersCheckWith(sequence, new_available, new_holding, new_requesting)

\* Banker's check with modified state
BankersCheckWith(seq, avail, hold, req) ==
    IF Len(seq) = 0 THEN TRUE
    ELSE
        LET p == Head(seq)
            need == req[p] \ hold[p]
        IN
            /\ need \subseteq avail
            /\ BankersCheckWith(Tail(seq), avail \cup hold[p], hold, req)

\* Next state relation
Next ==
    \/ \E p \in Processes, r \in Resources : RequestResource(p, r)
    \/ \E p \in Processes, r \in Resources : AllocateResource(p, r)
    \/ \E p \in Processes, r \in Resources : ReleaseResource(p, r)

\* Specification
Spec == Init /\ [][Next]_vars

\* Type correctness
TypeOK ==
    /\ holding \in ResourceState
    /\ requesting \in ResourceState
    /\ available \subseteq Resources

\* Safety properties
NoDeadlock == []~HasDeadlock
SystemSafety == []SafeState

\* Liveness properties
EventualProgress ==
    \A p \in Processes :
        requesting[p] # {} ~> requesting[p] = {}

ResourceEventuallyAvailable ==
    \A r \in Resources :
        r \notin available ~> r \in available

====