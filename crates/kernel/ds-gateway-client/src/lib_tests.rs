use super::*;

#[test]
fn an_invalid_path_never_produces_a_route() {
    let operation = Operation::ReadPrincipalDevice;
    assert!(matches!(
        operation.plan(&[]),
        Err(PlanError::MissingParameter("device_id"))
    ));
    assert!(matches!(
        operation.plan(&[("project", "other")]),
        Err(PlanError::UnknownParameter(_))
    ));
    assert!(matches!(
        operation.plan(&[("device_id", "a"), ("device_id", "b")]),
        Err(PlanError::DuplicateParameter(_))
    ));
    assert!(matches!(
        operation.plan(&[("device_id", "line\nbreak")]),
        Err(PlanError::InvalidParameter("device_id"))
    ));
    assert!(matches!(
        operation.plan(&[("device_id", "..")]),
        Err(PlanError::InvalidParameter("device_id"))
    ));
}

#[test]
fn a_path_parameter_cannot_escape_into_another_route_or_query() {
    let route = Operation::ReadPrincipalDevice
        .plan(&[("device_id", "a/b?project=other#fragment")])
        .unwrap();
    assert_eq!(
        route.path(),
        "/api/v1/auth/devices/a%2Fb%3Fproject%3Dother%23fragment"
    );
    assert_eq!(route.method(), Method::Get);
}

#[test]
fn typed_dispatch_is_closed_and_const_coordinates_preserve_existing_reads() {
    const METHOD: Method = Operation::ListUserProjects.method();
    const PATH: &str = Operation::ListUserProjects.path_template();
    assert_eq!(METHOD.as_str(), "GET");
    assert_eq!(PATH, "/api/v1/user/projects");
    assert_eq!(Operation::from_id("not_a_route"), None);
    assert_eq!(Operation::ALL.len(), 127);
    for operation in Operation::ALL {
        assert_eq!(
            Operation::from_id(operation.operation_id()),
            Some(*operation)
        );
    }
}

#[test]
fn the_policy_owner_receives_captured_context_once_and_keeps_its_exact_refusal() {
    struct ExistingPolicy {
        calls: usize,
    }
    impl Policy for ExistingPolicy {
        type Context = (&'static str, &'static str);
        type Output = Result<(), (&'static str, u16)>;

        fn exchange(&mut self, context: &Self::Context, route: RoutePlan) -> Self::Output {
            self.calls += 1;
            assert_eq!(*context, ("principal-a", "project-a"));
            assert_eq!(route.operation(), Operation::ListUserProjects);
            Err(("OWNER_CHANGED", 409))
        }
    }
    let mut client = Client::new(ExistingPolicy { calls: 0 });
    let result = client.call(
        &("principal-a", "project-a"),
        Operation::ListUserProjects.plan(&[]).unwrap(),
    );
    assert_eq!(result, Err(("OWNER_CHANGED", 409)));
    assert_eq!(client.into_policy().calls, 1);
}
