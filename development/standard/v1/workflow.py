"""Pure workflow admission. Storage/Git/proof loading are supplied by adapters."""
from copy import deepcopy

STAGES=('design','implementation','independent_verification','operational_application','effectiveness_confirmation')
KINDS=dict(zip(STAGES,('commit','check','review','application','effectiveness')))
STATES=('not_started','in_progress','verified')


def intent(record):
    return {k:v for k,v in record.items() if k not in ('stages','status','design_commit','unapplied')}


def validate(record,subject,load_proof,verify_design,verify_check):
    errors=[];stages=record.get('stages',{})
    if record.get('status')not in ['registered','in_progress','on_hold','complete']:errors.append('unknown workflow status')
    if not str(record.get('owner','')).startswith('parent'):errors.append('parent completion owner required')
    if set(stages)!=set(STAGES):return errors+['workflow stage set']
    for i,name in enumerate(STAGES):
        stage=stages[name];state=stage.get('state');evidence=stage.get('evidence')
        if state not in STATES or not isinstance(evidence,list):errors.append(name+': stage declaration');continue
        if state=='not_started' and evidence:errors.append(name+': unstarted stage has evidence')
        if state!='verified':continue
        if any(stages[p].get('state')!='verified' for p in STAGES[:i]):errors.append(name+': prerequisite not verified')
        if not evidence:errors.append(name+': verified stage needs evidence');continue
        for ref in evidence:
            try:
                if not isinstance(ref,dict)or ref.get('kind')!=KINDS[name]:raise ValueError('wrong evidence kind')
                if name=='design':
                    if not verify_design(ref,record):raise ValueError('current design not fixed in prior commit')
                    continue
                proof=load_proof(ref)
                if proof.get('change')!=record['id']or proof.get('subject_digest')!=subject or proof.get('stage')!=name:
                    raise ValueError('wrong evidence stage/subject')
                if name=='implementation':
                    if not verify_check(proof):raise ValueError('check evidence rejected')
                elif name=='independent_verification':
                    if proof.get('owner')!='parent'or proof.get('accepted')is not True or not proof.get('reviewer')or proof['reviewer']==record.get('implementer'):
                        raise ValueError('independent parent acceptance required')
                elif name=='operational_application':
                    if proof.get('owner')!='parent'or proof.get('applied')is not True or not proof.get('active_root'):
                        raise ValueError('application proof required')
                elif name=='effectiveness_confirmation':
                    if proof.get('owner')!='parent'or proof.get('confirmed')is not True or not proof.get('refusal_evidence'):
                        raise ValueError('effectiveness proof required')
            except (OSError,ValueError,KeyError,TypeError)as problem:errors.append(name+': '+str(problem))
    if record.get('status')=='complete'and any(stages[p].get('state')!='verified'for p in STAGES):errors.append('incomplete workflow cannot close')
    return errors


def admit(record,action,subject,load_proof,verify_design,verify_check):
    errors=validate(record,subject,load_proof,verify_design,verify_check)
    needed={'develop':('design',),'build':('design',),'stage':('design',),
            'ship':STAGES[:3],'close':STAGES}.get(action)
    if needed is None:return errors+['unknown workflow action']
    errors += [action+': '+s+' not verified'for s in needed if record['stages'][s].get('state')!='verified']
    return errors


def transition(record,stage,state,evidence,actor,subject,load_proof,verify_design,verify_check):
    if stage not in STAGES or state not in STATES:raise ValueError('unknown workflow transition')
    old=record['stages'][stage]['state']
    if STATES.index(state)<STATES.index(old)or STATES.index(state)>STATES.index(old)+1:raise ValueError('invalid workflow transition')
    if stage in STAGES[2:]and actor!='parent':raise ValueError('parent owns independent/application/closure stages')
    updated=deepcopy(record);updated['stages'][stage]=dict(state=state,evidence=evidence)
    errors=validate(updated,subject,load_proof,verify_design,verify_check)
    if errors:raise ValueError('; '.join(errors))
    return updated
