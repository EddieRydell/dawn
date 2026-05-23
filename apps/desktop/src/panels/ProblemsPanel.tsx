import { useWorkbench } from "../store/workbenchStore";

export function ProblemsPanel() {
  const problems = useWorkbench((state) => state.languageProblems);
  const openProblem = useWorkbench((state) => state.openProblem);

  return (
    <section className="panel problems">
      {problems.length === 0 ? (
        <div className="empty-problems">No problems reported.</div>
      ) : (
        problems.map((problem, index) => (
          <button
            key={`${problem.path}:${problem.line}:${problem.column}:${index}`}
            className={`problem ${problem.severity.toLowerCase()}`}
            onClick={() => void openProblem(problem)}
          >
            <strong>{problem.severity}</strong>
            <span>{problem.message}</span>
            <small>
              {problem.path}:{problem.line}:{problem.column}
            </small>
          </button>
        ))
      )}
    </section>
  );
}
