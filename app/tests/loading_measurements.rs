//! CLI correctness fixtures only: these records are not measured player performance.
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture(PathBuf);

impl Fixture {
    fn new(mutation: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "beatkernel-loading-report-fixture-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Self(directory);
        let generated = Command::new("python3")
            .arg("-c")
            .arg(format!("{GENERATOR}\n{mutation}\nimport json,sys\nwith open(sys.argv[1],'w') as f: json.dump(data,f)"))
            .arg(fixture.0.join("trials.json"))
            .output()
            .expect("Python is required by the loading report CLI");
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        fixture
    }

    fn report(&self) -> Output {
        Command::new("python3")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/loading_report.py"))
            .arg(self.0.join("trials.json"))
            .arg("--output")
            .arg(self.0.join("report.json"))
            .output()
            .expect("run actual loading report CLI")
    }

    fn assert_report(&self, assertions: &str) {
        let result = self.report();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let checked = Command::new("python3")
            .arg("-c")
            .arg(format!(
                "import json,sys\nwith open(sys.argv[1]) as f: report=json.load(f)\n{assertions}"
            ))
            .arg(self.0.join("report.json"))
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{}",
            String::from_utf8_lossy(&checked.stderr)
        );
    }

    fn assert_refused(&self) {
        let result = self.report();
        assert!(
            !result.status.success(),
            "invalid external evidence was accepted"
        );
        assert!(
            !self.0.join("report.json").exists(),
            "refusal published a report"
        );
        assert!(
            !result.stderr.is_empty(),
            "refusal must explain the invalid evidence"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// Real-shaped external event records, explicitly synthetic and never performance proof.
const GENERATOR: &str = r#"
sha='a'*64
artifact='b'*64
def group(variant,temperature,n=5):
    trials=[]
    for i in range(n):
        owner=f'{variant}-{temperature}-{i}' if temperature=='cold' else f'{variant}-warm-owner'
        correlation=f'{variant}-{temperature}-request-{i}'
        def event(name,time):
            return dict(name=name,at_ms=time,correlation_id=correlation,source='external-fixture-observer')
        trials.append(dict(trial_id=f'{variant}-{temperature}-trial-{i}',process_id=owner,library_id=owner,
            cache_owner_id=owner,correlation_id=correlation,
            reused_owner_id=None if temperature=='cold' else owner,
            events=[event('request',0),event('catalog_selectable',1),
                event('prepared_ready',2),event('audio_ready',3),
                event('first_presented_usable',(i+1)*10)],
            loaded_artifacts={'/player.wasm':artifact},
            phases_ms={'parse':2,'decode':4},counters={'reads':1,'decodes':1},
            input_latency_ms=[0,1],redraw_latency_ms=[2]))
    return dict(variant=variant,scenario='same-chart',temperature=temperature,
        fixture_sha256=sha,config_sha256=sha,source_revision='1234567',
        artifact_manifest={'/player.wasm':artifact},trials=trials)
data=dict(schema_version=1,workload_id='synthetic-cli-contract-fixture',path='native',
    environment={'fixture_only':True,'performance_proof':False},
    fixture_sha256=sha,config_sha256=sha,
    groups=[group(v,t) for t in ('cold','warm') for v in ('before','after')])
"#;

#[test]
fn nearest_rank_reports_actual_event_distributions_and_separate_readiness() {
    Fixture::new("").assert_report(
        r#"
assert report['schema_version']==1
assert report['quantile_method']=='nearest-rank'
assert len(report['comparisons'])==2
for comparison in report['comparisons']:
    for variant in ('before','after'):
        events=comparison[variant]['events_ms']
        assert events['first_presented_usable']==dict(samples=5,p50=30,p95=50,max=50)
        assert events['catalog_selectable']==dict(samples=5,p50=1,p95=1,max=1)
        assert events['prepared_ready']['p50']==2
        assert comparison[variant]['counters']['reads']['p50']==1
"#,
    );
}

#[test]
fn nearest_rank_boundary_uses_nineteenth_sample_for_twenty_trials() {
    Fixture::new("data['groups']=[group(v,t,20) for t in ('cold','warm') for v in ('before','after')]")
        .assert_report(r#"
for comparison in report['comparisons']:
    for variant in ('before','after'):
        assert comparison[variant]['events_ms']['first_presented_usable']==dict(samples=20,p50=100,p95=190,max=200)
"#);
}

#[test]
fn fewer_than_five_trials_and_unpaired_workloads_are_refused() {
    for mutation in [
        "data['groups'][0]['trials'].pop()",
        "data['groups'].pop()",
        "data['groups'][1]['scenario']='changed-chart'",
    ] {
        Fixture::new(mutation).assert_refused();
    }
}

#[test]
fn event_correlation_identity_and_readiness_order_are_required() {
    for mutation in [
        "data['groups'][1]['trials'][0]['trial_id']=data['groups'][0]['trials'][0]['trial_id']",
        "data['groups'][0]['trials'][0]['events'][1]['correlation_id']='other-request'",
        "data['groups'][0]['trials'][0]['events'][0]['at_ms']=1",
        "data['groups'][0]['trials'][0]['events'][-1]['at_ms']=0.5",
        "data['groups'][0]['trials'][0]['events'][3]['at_ms']=1.5",
        "data['groups'][0]['trials'][0]['events'].pop()",
        "data['groups'][0]['trials'][0]['events'].append(data['groups'][0]['trials'][0]['events'][1].copy())",
        "data['groups'][0]['trials'][0]['events'][1]['name']='invented-readiness'",
    ] {
        Fixture::new(mutation).assert_refused();
    }
}

#[test]
fn fixture_configuration_and_actual_loaded_artifact_provenance_are_checked() {
    for mutation in [
        "data['groups'][0]['fixture_sha256']='c'*64",
        "data['groups'][0]['config_sha256']='c'*64",
        "data['groups'][0]['source_revision']='unknown-build'",
        "data['groups'][0]['trials'][0]['loaded_artifacts']['/player.wasm']='c'*64",
        "data['groups'][0]['trials'][0]['loaded_artifacts']['/unfrozen-worker.js']='c'*64",
    ] {
        Fixture::new(mutation).assert_refused();
    }
}

#[test]
fn cold_owners_are_fresh_and_warm_reuse_declares_exact_owner() {
    for mutation in [
        "data['groups'][0]['trials'][1]['process_id']=data['groups'][0]['trials'][0]['process_id']",
        "data['groups'][0]['trials'][1]['library_id']=data['groups'][0]['trials'][0]['library_id']",
        "data['groups'][0]['trials'][1]['cache_owner_id']=data['groups'][0]['trials'][0]['cache_owner_id']",
        "data['groups'][0]['trials'][0]['reused_owner_id']='old-owner'",
        "data['groups'][2]['trials'][0]['reused_owner_id']=None",
        "data['groups'][2]['trials'][0]['reused_owner_id']='other-owner'",
    ] {
        Fixture::new(mutation).assert_refused();
    }
}

#[test]
fn unavailable_baseline_phase_and_counter_cannot_invent_speedup() {
    Fixture::new(
        r#"
for g in data['groups']:
    if g['variant']=='before':
        for trial in g['trials']:
            trial['phases_ms']['decode']=None
            del trial['counters']['decodes']
"#,
    )
    .assert_report(
        r#"
for comparison in report['comparisons']:
    for key in ('phases_ms/decode','counters/decodes'):
        metric=comparison['metrics'][key]
        assert metric['comparable'] is False
        assert metric['reason']
        assert metric['before']==dict(samples=0,p50=None,p95=None,max=None)
        assert metric['after']['samples']==5
        for delta in ('p50_delta','p95_delta','max_delta'):
            assert metric.get(delta) is None
    assert comparison['metrics']['phases_ms/parse']['comparable'] is True
"#,
    );
}

#[test]
fn partial_unavailable_baseline_is_also_incomparable_and_invalid_samples_refuse() {
    Fixture::new("data['groups'][0]['trials'][0]['phases_ms']['parse']=None").assert_report(
        r#"
comparison=next(c for c in report['comparisons'] if c['temperature']=='cold')
metric=comparison['metrics']['phases_ms/parse']
assert metric['comparable'] is False
assert metric['before']['samples']==4
assert metric.get('p50_delta') is None
"#,
    );
    for mutation in [
        "data['groups'][0]['trials'][0]['phases_ms']['parse']=-1",
        "data['groups'][0]['trials'][0]['counters']['reads']=1.5",
        "data['groups'][0]['trials'][0]['input_latency_ms']=[-1]",
        "data['groups'][0]['trials'][0]['events'][-1]['at_ms']=float('inf')",
    ] {
        Fixture::new(mutation).assert_refused();
    }
}
