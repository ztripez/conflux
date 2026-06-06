use conflux_ir::{Cadence, ValueKind};
use conflux_kernel::{
    execute_actor_rule, ActorInputSource, ActorKernel, ActorKernelBinding, Assessment, KernelExpr,
    ScalarType,
};
use conflux_wgsl::{emit_actor_wgsl, lower_actor_kernels, Access, ActorBindingSource, WgslError};

fn actor_kernel() -> ActorKernel {
    ActorKernel {
        name: "graze".to_string(),
        actor_set: 0,
        actor_set_name: "herd".to_string(),
        field: 1,
        count: 3,
        target: 0,
        target_name: "energy".to_string(),
        cadence: Cadence { period: 1 },
        scalar_type: ScalarType::F32,
        bindings: vec![
            ActorKernelBinding {
                name: "energy".to_string(),
                source: ActorInputSource::ActorChannel(0),
                kind: ValueKind::Stock,
            },
            ActorKernelBinding {
                name: "grass".to_string(),
                source: ActorInputSource::FieldSample(2),
                kind: ValueKind::Stock,
            },
        ],
        expr: KernelExpr::Add(
            Box::new(KernelExpr::Input(0)),
            Box::new(KernelExpr::Mul(
                Box::new(KernelExpr::Input(1)),
                Box::new(KernelExpr::Literal(0.25)),
            )),
        ),
        diagnostics: vec![Assessment::Range {
            min: 0.0,
            max: 10.0,
        }],
    }
}

#[test]
fn emits_stable_actor_wgsl_with_position_sample_metadata() {
    let kernel = actor_kernel();

    let module = emit_actor_wgsl(&kernel).unwrap();

    assert_eq!(module.kernel, "graze");
    assert_eq!(module.actor_count, 3);
    assert_eq!(module.target, 0);
    assert!(module.source.contains("v_grass[v_positions[i]]"));
    assert!(module.source.contains("v_energy[i] = out;"));
    assert_eq!(module.bindings.len(), 4);
    assert_eq!(module.bindings[0].access, Access::Read);
    assert_eq!(
        module.bindings[0].source,
        ActorBindingSource::FieldSample {
            field_index: 1,
            name: "grass".to_string(),
            channel: 2,
        }
    );
    assert_eq!(module.bindings[1].access, Access::ReadWrite);
    assert_eq!(module.bindings[2].source, ActorBindingSource::Positions);
    assert_eq!(
        module.bindings[3].source,
        ActorBindingSource::Diagnostics { assessments: 1 }
    );

    assert_eq!(emit_actor_wgsl(&kernel).unwrap().source, module.source);
}

#[test]
fn emitted_actor_expression_matches_cpu_actor_kernel_inputs() {
    let kernel = actor_kernel();
    let actor_channels = vec![vec![1.0, 2.0, 3.0]];
    let field_channels = vec![vec![], vec![], vec![4.0, 8.0, 12.0, 16.0]];
    let positions = vec![2, 0, 3];

    let cpu = execute_actor_rule(&kernel, &actor_channels, &field_channels, &positions);

    assert_eq!(cpu, vec![4.0, 3.0, 7.0]);
    assert!(emit_actor_wgsl(&kernel)
        .unwrap()
        .source
        .contains("(v_energy[i] + (v_grass[v_positions[i]] * 0.25))"));
}

#[test]
fn rejects_actor_expression_with_missing_input_binding() {
    let mut kernel = actor_kernel();
    kernel.expr = KernelExpr::Input(99);

    assert!(matches!(
        emit_actor_wgsl(&kernel),
        Err(WgslError::InvalidActorInput {
            input: 99,
            available_inputs: 2,
            ..
        })
    ));
}

#[test]
fn lower_actor_kernels_reports_acceptance_and_rejection() {
    let accepted = actor_kernel();
    let mut rejected = actor_kernel();
    rejected.name = "bad".to_string();
    rejected.expr = KernelExpr::Input(9);

    let report = lower_actor_kernels(&[accepted, rejected]);

    assert_eq!(report.accepted_actors.len(), 1);
    assert_eq!(report.rejected_actors.len(), 1);
    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 1);
    assert!(report.to_string().contains("LOWER ACTOR `graze`"));
    assert!(report.to_string().contains("REJECT ACTOR `bad`"));
}

#[cfg(feature = "gpu")]
#[test]
fn emitted_actor_wgsl_is_accepted_by_wgpu_shader_frontend() {
    let module = emit_actor_wgsl(&actor_kernel()).unwrap();
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    let Some(adapter) = adapter else {
        return;
    };
    let (device, _) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("conflux-actor-wgsl-validation"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
        },
        None,
    ))
    .unwrap();

    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("conflux-actor-wgsl"),
        source: wgpu::ShaderSource::Wgsl(module.source.into()),
    });
}
