---- MODULE information_flow ----
EXTENDS TLC, Integers, Sequences

CONSTANTS
    Principals,    \* Set of security principals (VMs, hypervisor)
    Resources,     \* Set of resources (memory, CPU, devices)
    SecurityLevels \* Security classification levels

VARIABLES
    access,        \* Access control matrix
    information,   \* Information flow tracking
    labels,        \* Security labels for resources
    flows          \* Actual information flows

vars == <<access, information, labels, flows>>

\* Security levels (simplified Bell-LaPadula model)
Levels == {"UNCLASSIFIED", "CONFIDENTIAL", "SECRET", "TOP_SECRET"}

\* Access permissions
Permissions == {"READ", "WRITE", "EXECUTE"}

\* Information flow security property (no read up, no write down)
BellLaPadula ==
    \A p \in Principals, r \in Resources :
        /\ access[p][r]["READ"] => labels[p] >= labels[r]
        /\ access[p][r]["WRITE"] => labels[p] <= labels[r]

\* Non-interference property
NonInterference ==
    \A p1, p2 \in Principals :
        labels[p1] # labels[p2] =>
            \A r \in Resources :
                access[p1][r]["WRITE"] => ~access[p2][r]["READ"]

\* Information flow tracking
FlowTracking ==
    \A f \in flows :
        /\ f.source \in Principals
        /\ f.destination \in Principals  
        /\ f.resource \in Resources
        /\ labels[f.source] <= labels[f.destination]

\* Initial state
Init ==
    /\ access = [p \in Principals |-> [r \in Resources |-> [perm \in Permissions |-> FALSE]]]
    /\ information = [p \in Principals |-> {}]
    /\ labels = [p \in Principals |-> "UNCLASSIFIED"]
    /\ flows = {}

\* Grant access permission
GrantAccess(principal, resource, permission) ==
    /\ access' = [access EXCEPT ![principal][resource][permission] = TRUE]
    /\ UNCHANGED <<information, labels, flows>>

\* Information flow
InformationFlow(source, dest, resource) ==
    /\ access[source][resource]["READ"]
    /\ access[dest][resource]["WRITE"]
    /\ labels[source] <= labels[dest]
    /\ flows' = flows \cup {[source |-> source, destination |-> dest, resource |-> resource]}
    /\ UNCHANGED <<access, information, labels>>

\* Next state relation
Next ==
    \/ \E p \in Principals, r \in Resources, perm \in Permissions : GrantAccess(p, r, perm)
    \/ \E s, d \in Principals, r \in Resources : InformationFlow(s, d, r)

\* Specification
Spec == Init /\ [][Next]_vars

\* Safety properties
TypeOK ==
    /\ access \in [Principals -> [Resources -> [Permissions -> BOOLEAN]]]
    /\ labels \in [Principals -> Levels]

\* Security properties
SecurityPolicy == []BellLaPadula
InformationFlowSecurity == []FlowTracking
NoInformationLeakage == []NonInterference

====