@0xf2c0f3a93e0203ec;

using ST = import "storage.capnp";

struct TableField {
	name @0 :Text;
	baseType @1 :Type;
	nullable @2 :Bool;
	enum Type {
		integer @0;
		real @1;
		text @2;
		blob @3;
		pointer @4; # a capnproto pointer, including capabilities; the database adapter layer will transparently save and restore these.
	}
}

struct DBAny {
    union {
	  null @0 :Void;
      integer @1 :Int64;
      real @2 :Float64;
      text @3 :Text;
      blob @4 :Data;
      pointer @5 :AnyPointer;
    }
}

interface ROTableRef {
}

interface RATableRef extends(ROTableRef) {
  readonly @0 () -> (res :ROTableRef);
}

interface TableRef extends(RATableRef) {
	appendonly @0 () -> (res :RATableRef);
}

interface Table extends(TableRef, ST.Saveable(Table)) {
  adminless @0 () -> (res :TableRef);
}

struct Storage {
	id @0 :UInt8;
	data @1 :UInt64;
}

interface Root extends(AddDB, ST.Restore(Storage)) {}

struct TableRestriction {
	name @0 :Text;
	value @1 :DBAny;
}

struct IndexedColumn {
	union {
		name @0 :Text;
		expr @1 :Expr;
	}
}

struct IndexDef {
	base @0 :TableRef;
	cols @1 :List(IndexedColumn); # must be nonempty
	sqlWhere @2 :List(Expr); # may be empty
}

interface Index {
}

interface AddDB extends(Database)  {
  createTable @0 (def :List(TableField)) -> (res :Table);
	createView @1 (names :List(Text), def :Select) -> (res :ROTableRef);

	createRestrictedTable @2 (base :Table, restriction :List(TableRestriction)) -> (res :Table);
	# Create a view that shows the contents of the base table but without the restricted fields, where the restricted field has the specified value.
	# It will have triggers instead of insert, update, and delete that forward the operations to the base table with the restricted fields set to the specified values.
	# this can be used to share a subset of a data table in a secure manner.

	createIndex @3 (base :TableRef, cols :List(IndexedColumn), sqlWhere :List(Expr)) -> (res :Index);
}

interface Database extends(RODatabase) {
	insert @0 (ins :Insert) -> StatementResults;
	prepareInsert @1 (ins :Insert) -> (stmt :PreparedStatement(Insert));
	runPreparedInsert @2 (stmt :PreparedStatement(Insert), bindings :List(DBAny)) -> (res :ResultStream);
	update @3 (upd :Update) -> StatementResults;
	prepareUpdate @4 (upd :Update) -> (stmt :PreparedStatement(Update));
	runPreparedUpdate @5 (stmt :PreparedStatement(Update), bindings :List(DBAny)) -> (res :ResultStream);
	delete @6 (del :Delete) -> StatementResults;
	prepareDelete @7 (del :Delete) -> (stmt :PreparedStatement(Delete));
	runPreparedDelete @8 (stmt :PreparedStatement(Delete), bindings :List(DBAny)) -> (res :ResultStream);
}

interface PreparedStatement(Clause) {
	# this doesn't necessarily correspond to a single sqlite prepared statement; the backend may create multiple to handle multiple invocations
	# but this avoids sending the same expression tree over rpc multiple times even if the sqlite engine needs to parse it multiple times because their design is bad.
}

interface RODatabase {
  select @0 (q :Select) -> StatementResults;
  prepareSelect @1 (q :Select) -> (stmt :PreparedStatement(Select));
  runPreparedSelect @2 (stmt :PreparedStatement(Select), bindings :List(DBAny)) -> (res :ResultStream);
}

struct StatementResults {
	res @0 :ResultStream;
}

interface ResultStream {
  next @0 (size :UInt16) -> (res :StreamResult);
}

struct StreamResult {
  results @0 :List(List(DBAny));
  finished @1 :Bool;
}

interface SqlFunction {
	# all the functions are caps
}

struct FunctionInvocation {
	function @0 :SqlFunction;
  params @1 :List(Expr);
}

struct Expr {
	# an ordinary expression in a sql operation
	struct TableColumn {
		# read a column from a table (or table like object) in scope
		colName @0 :Text;
		reference @1 :UInt16; #debruijn level - comes from the preorder traversal of the join tree
	}
	enum Operator {
		is @0;
		isNot @1;
		and @2;
		or @3;
	}

	union {
		literal @0 :DBAny;
		bindparam @1 :Void;
		column @2 :TableColumn;
		functioninvocation @3 :FunctionInvocation;
		op @4 :Operator;
	}
}

struct Select {
	struct MergeOperation {
		enum MergeOperator {
			union @0;
			unionall @1;
			intersect @2;
			except @3;
		}

		operator @0 :MergeOperator;
		selectcore @1 :SelectCore;
	}

	names @4 :List(Text);
	# Optional; if the select is being returned to you this has no semantic meaning, but might be necessary to use when nesting this select in other expressions.
	# If an entry in the list is the empty string or the entire list is null, the result will be given a name arbitrarily
	# Every individual select core clause can have names, but they don't actually matter and the results are actually positional, but names must exist for nested operations
	# for example `SELECT 1 AS x, "foo" AS y UNION SELECT "bar" AS y, 2 AS x` returns two rows with the following structure: {x = 1, y = "foo"}, {x = "bar", y = 2}

	selectcore @0 :SelectCore;
	mergeoperations @1 :List(MergeOperation);

	struct OrderingTerm {
		enum AscDesc {
			asc @0;
			desc @1;
		}

		# enum CollationName {
		#   # there should be something here
		#   # this is supposed to be about the nightmarish world of sorting strings
		# }

		expr @0 :Expr;
		# what expression to order by, please don't ask for string collated orders yet

		# collate @1 :CollationName; # even dragons fear to tread upon this barren earth. here don't be dragons.
		direction @1 :AscDesc;
	}

	orderby @2 :List(OrderingTerm);
	# order the results, the first one has highest precedence
	# may be null, if null then the generated sql will not have a limit clause

	struct LimitOperation {
		# must be constant
		limit @0 :Expr;
		offset @1 :Expr;
	}

	limit @3 :LimitOperation; # may be null, if null then the generated sql will not have a limit clause
}

struct SelectCore {
 	from @0 :JoinClause; # May be null; if null then the results must be constant expressions and generated select will not have a from clause
  results @1 :List(Expr); # Must be provided
  sqlWhere @2 :List(Expr); # may be empty; if empty then the generated select will not have a where clause; must be a boolean valued expression
}


struct JoinClause {
  struct JoinOperation {
    struct JoinConstraint {
	  	union {
				expr @0 :Expr; #must have 
				cols @1 :List(Text);
				empty @2 :Void;
			}
    }
    struct JoinOperator {
      enum JoinParameter {
				# what to do when things don't match
				left @0;
				right @1;
				full @2;
				none @3;
			}

      union {
				innerJoin @0 :Void;
				outerJoin @1 :JoinParameter;
				plainJoin @2 :JoinParameter;
			}
    }

    operator @0 :JoinOperator;
		tableorsubquery @1 :TableOrSubquery;
		joinconstraint @2 :JoinConstraint;
  }

  tableorsubquery @0 :TableOrSubquery;
  joinoperations @1 :List(JoinOperation);
}

interface TableFunctionRef {
}

struct TableOrSubquery {
  struct TableFunctionInvocation {
    functionref @0 :TableFunctionRef;
		exprs @1 :List(Expr);
  }

  union {
    tableref @0 :ROTableRef;
    tablefunctioninvocation @1 :TableFunctionInvocation;
		select @2 :Select;
		joinclause @3 :JoinClause;
		null @4 :Void;

  }
}

struct Insert {
	# Append an entry to a table
	enum ConflictStrategy {
		# not all of sqlite's strategies are allowed here to ensure that this operation can't change an existing row
		abort @0;
		fail @1;
		ignore @2;
		rollback @3;
	}

	fallback @0 :ConflictStrategy;
	target @1 :RATableRef;
	cols @2 :List(Text);
	source :union {
		values @3 :List(List(DBAny));
		select @4 :Select;
		defaults @5 :Void;
	}
	returning @6 :List(Expr); # optional, may be null.
}

struct Update {
	enum ConflictStrategy {
		abort @0;
		fail @1;
		ignore @2;
		rollback @3;
		replace @4;
	}

	struct Assignment {
		name @0 :Text;
		expr @1 :Expr;
	}

	fallback @0 :ConflictStrategy;
	assignments @1 :List(Assignment);
	from @2 :JoinClause;
	sqlWhere @3 :List(Expr);
	returning @4 :List(Expr);
}

struct Delete {
	from @0 :TableRef;
	sqlWhere @1 :List(Expr);
	returning @2 :List(Expr);
}